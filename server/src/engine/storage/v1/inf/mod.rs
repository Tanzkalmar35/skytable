/*
 * Created on Fri Aug 04 2023
 *
 * This file is a part of Skytable
 * Skytable (formerly known as TerrabaseDB or Skybase) is a free and open-source
 * NoSQL database written by Sayan Nandan ("the Author") with the
 * vision to provide flexibility in data modelling without compromising
 * on performance, queryability or scalability.
 *
 * Copyright (c) 2023, Sayan Nandan <ohsayan@outlook.com>
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program. If not, see <https://www.gnu.org/licenses/>.
 *
*/

//! High level interfaces

use crate::engine::idx::STIndex;

mod map;
mod obj;
// tests
#[cfg(test)]
mod tests;

use {
    crate::engine::{
        data::{
            dict::DictEntryGeneric,
            tag::{DataTag, TagClass},
        },
        idx::{AsKey, AsValue},
        storage::v1::{rw::BufferedScanner, SDSSError, SDSSResult},
    },
    std::mem,
};

type VecU8 = Vec<u8>;

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, sky_macros::EnumMethods)]
#[repr(u8)]
/// Disambiguation for data
pub enum PersistDictEntryDscr {
    Null = 0,
    Bool = 1,
    UnsignedInt = 2,
    SignedInt = 3,
    Float = 4,
    Bin = 5,
    Str = 6,
    List = 7,
    Dict = 8,
}

impl PersistDictEntryDscr {
    /// translates the tag class definition into the dscr definition
    pub const fn translate_from_class(class: TagClass) -> Self {
        unsafe { Self::from_raw(class.d() + 1) }
    }
    pub const unsafe fn from_raw(v: u8) -> Self {
        core::mem::transmute(v)
    }
    pub fn new_from_dict_gen_entry(e: &DictEntryGeneric) -> Self {
        match e {
            DictEntryGeneric::Map(_) => Self::Dict,
            DictEntryGeneric::Lit(dc) => Self::translate_from_class(dc.tag().tag_class()),
        }
    }
    /// The data in question is null (well, can we call that data afterall?)
    pub const fn is_null(&self) -> bool {
        self.value_u8() == Self::Null.value_u8()
    }
    /// The data in question is a scalar
    pub const fn is_scalar(&self) -> bool {
        self.value_u8() <= Self::Float.value_u8()
    }
    /// The data is composite
    pub const fn is_composite(&self) -> bool {
        self.value_u8() > Self::Float.value_u8()
    }
    /// Recursive data
    pub const fn is_recursive(&self) -> bool {
        self.value_u8() >= Self::List.value_u8()
    }
    fn into_class(&self) -> TagClass {
        debug_assert!(*self != Self::Null);
        unsafe { mem::transmute(self.value_u8() - 1) }
    }
}

/*
    md spec
*/

/// metadata spec for a persist map entry
pub trait PersistObjectMD: Sized {
    /// set to true if decode is infallible once the MD payload has been verified
    const MD_DEC_INFALLIBLE: bool;
    /// returns true if the current buffered source can be used to decode the metadata (self)
    fn pretest_src_for_metadata_dec(scanner: &BufferedScanner) -> bool;
    /// returns true if per the metadata and the current buffered source, the target object in question can be decoded
    fn pretest_src_for_object_dec(&self, scanner: &BufferedScanner) -> bool;
    /// decode the metadata
    unsafe fn dec_md_payload(scanner: &mut BufferedScanner) -> Option<Self>;
}

/// Metadata for a simple size requirement
pub struct SimpleSizeMD<const N: usize>;

impl<const N: usize> PersistObjectMD for SimpleSizeMD<N> {
    const MD_DEC_INFALLIBLE: bool = true;
    fn pretest_src_for_metadata_dec(scanner: &BufferedScanner) -> bool {
        scanner.has_left(N)
    }
    fn pretest_src_for_object_dec(&self, _: &BufferedScanner) -> bool {
        true
    }
    unsafe fn dec_md_payload(_: &mut BufferedScanner) -> Option<Self> {
        Some(Self)
    }
}

/// For wrappers and other complicated metadata handling, set this to the metadata type
pub struct VoidMetadata;

impl PersistObjectMD for VoidMetadata {
    const MD_DEC_INFALLIBLE: bool = true;
    fn pretest_src_for_metadata_dec(_: &BufferedScanner) -> bool {
        true
    }
    fn pretest_src_for_object_dec(&self, _: &BufferedScanner) -> bool {
        true
    }
    unsafe fn dec_md_payload(_: &mut BufferedScanner) -> Option<Self> {
        Some(Self)
    }
}

/// Decode metadata
///
/// ## Safety
/// unsafe because you need to set whether you've already verified the metadata or not
unsafe fn dec_md<Md: PersistObjectMD, const ASSUME_PRETEST_PASS: bool>(
    scanner: &mut BufferedScanner,
) -> SDSSResult<Md> {
    if ASSUME_PRETEST_PASS || Md::pretest_src_for_metadata_dec(scanner) {
        match Md::dec_md_payload(scanner) {
            Some(md) => Ok(md),
            None => {
                if Md::MD_DEC_INFALLIBLE {
                    impossible!()
                } else {
                    Err(SDSSError::InternalDecodeStructureCorrupted)
                }
            }
        }
    } else {
        Err(SDSSError::InternalDecodeStructureCorrupted)
    }
}

/*
    obj spec
*/

/// Specification for any object that can be persisted
///
/// To actuall enc/dec any object, use functions (and their derivatives) [`enc`] and [`dec`]
pub trait PersistObjectHlIO {
    const ALWAYS_VERIFY_PAYLOAD_USING_MD: bool;
    /// the actual type (we can have wrappers)
    type Type;
    /// the metadata type (use this to verify the buffered source)
    type Metadata: PersistObjectMD;
    /// enc routine
    ///
    /// METADATA: handle yourself
    fn pe_obj_hlio_enc(buf: &mut VecU8, v: &Self::Type);
    /// dec routine
    unsafe fn pe_obj_hlio_dec(
        scanner: &mut BufferedScanner,
        md: Self::Metadata,
    ) -> SDSSResult<Self::Type>;
}

/// enc the given object into a new buffer
pub fn enc<Obj: PersistObjectHlIO>(obj: &Obj::Type) -> VecU8 {
    let mut buf = vec![];
    Obj::pe_obj_hlio_enc(&mut buf, obj);
    buf
}

/// enc the object into the given buffer
pub fn enc_into_buf<Obj: PersistObjectHlIO>(buf: &mut VecU8, obj: &Obj::Type) {
    Obj::pe_obj_hlio_enc(buf, obj)
}

/// enc the object into the given buffer
pub fn enc_self_into_buf<Obj: PersistObjectHlIO<Type = Obj>>(buf: &mut VecU8, obj: &Obj) {
    Obj::pe_obj_hlio_enc(buf, obj)
}

/// enc the object into a new buffer
pub fn enc_self<Obj: PersistObjectHlIO<Type = Obj>>(obj: &Obj) -> VecU8 {
    enc::<Obj>(obj)
}

/// dec the object
pub fn dec<Obj: PersistObjectHlIO>(scanner: &mut BufferedScanner) -> SDSSResult<Obj::Type> {
    if Obj::Metadata::pretest_src_for_metadata_dec(scanner) {
        let md = unsafe {
            // UNSAFE(@ohsaya): pretest
            dec_md::<Obj::Metadata, true>(scanner)?
        };
        if Obj::ALWAYS_VERIFY_PAYLOAD_USING_MD && !md.pretest_src_for_object_dec(scanner) {
            return Err(SDSSError::InternalDecodeStructureCorrupted);
        }
        unsafe { Obj::pe_obj_hlio_dec(scanner, md) }
    } else {
        Err(SDSSError::InternalDecodeStructureCorrupted)
    }
}

/// dec the object
pub fn dec_self<Obj: PersistObjectHlIO<Type = Obj>>(
    scanner: &mut BufferedScanner,
) -> SDSSResult<Obj> {
    dec::<Obj>(scanner)
}

/*
    map spec
*/

/// specification for a persist map
pub trait PersistMapSpec {
    type MapType: STIndex<Self::Key, Self::Value>;
    /// metadata type
    type EntryMD: PersistObjectMD;
    /// key type (NOTE: set this to the true key type; handle any differences using the spec unless you have an entirely different
    /// wrapper type)
    type Key: AsKey;
    /// value type (NOTE: see [`PersistMapSpec::Key`])
    type Value: AsValue;
    /// coupled enc
    const ENC_COUPLED: bool;
    /// coupled dec
    const DEC_COUPLED: bool;
    /// verify the src using the given metadata
    const META_VERIFY_BEFORE_DEC: bool;
    // collection meta
    /// pretest before jmp to routine for entire collection
    fn meta_dec_collection_pretest(scanner: &BufferedScanner) -> bool;
    /// pretest before jmp to entry dec routine
    fn meta_dec_entry_pretest(scanner: &BufferedScanner) -> bool;
    // entry meta
    /// enc the entry meta
    fn entry_md_enc(buf: &mut VecU8, key: &Self::Key, val: &Self::Value);
    /// dec the entry meta
    /// SAFETY: ensure that all pretests have passed (we expect the caller to not be stupid)
    unsafe fn entry_md_dec(scanner: &mut BufferedScanner) -> Option<Self::EntryMD>;
    // independent packing
    /// enc key (non-packed)
    fn enc_key(buf: &mut VecU8, key: &Self::Key);
    /// enc val (non-packed)
    fn enc_val(buf: &mut VecU8, key: &Self::Value);
    /// dec key (non-packed)
    unsafe fn dec_key(scanner: &mut BufferedScanner, md: &Self::EntryMD) -> Option<Self::Key>;
    /// dec val (non-packed)
    unsafe fn dec_val(scanner: &mut BufferedScanner, md: &Self::EntryMD) -> Option<Self::Value>;
    // coupled packing
    /// entry packed enc
    fn enc_entry(buf: &mut VecU8, key: &Self::Key, val: &Self::Value);
    /// entry packed dec
    unsafe fn dec_entry(
        scanner: &mut BufferedScanner,
        md: Self::EntryMD,
    ) -> Option<(Self::Key, Self::Value)>;
}
