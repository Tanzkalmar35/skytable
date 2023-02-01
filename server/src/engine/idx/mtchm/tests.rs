/*
 * Created on Sun Jan 29 2023
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

use super::{
    super::{
        super::sync::atm::{cpin, Guard},
        IndexBaseSpec, MTIndex,
    },
    imp::ChmCopy as _ChmCopy,
    meta::DefConfig,
};
use std::{
    hash::{BuildHasher, Hasher},
    sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard},
    thread::{self, JoinHandle},
    time::Duration,
};

type Chm<K, V> = ChmCopy<K, V>;
type ChmCopy<K, V> = _ChmCopy<K, V, DefConfig>;

struct LolHash {
    seed: usize,
}

impl LolHash {
    const fn with_seed(seed: usize) -> Self {
        Self { seed }
    }
    const fn init_default_seed() -> Self {
        Self::with_seed(0)
    }
}

impl Default for LolHash {
    fn default() -> Self {
        Self::init_default_seed()
    }
}

impl Hasher for LolHash {
    fn finish(&self) -> u64 {
        self.seed as _
    }
    fn write(&mut self, _: &[u8]) {}
}

struct LolState {
    seed: usize,
}

impl BuildHasher for LolState {
    type Hasher = LolHash;

    fn build_hasher(&self) -> Self::Hasher {
        LolHash::with_seed(self.seed)
    }
}

impl Default for LolState {
    fn default() -> Self {
        Self { seed: 0 }
    }
}

type ChmU8 = Chm<u8, u8>;

// empty
#[test]
fn drop_empty() {
    let idx = ChmU8::idx_init();
    drop(idx);
}

#[test]
fn get_empty() {
    let idx = ChmU8::idx_init();
    assert!(idx.mt_get(&10, &cpin()).is_none());
}

#[test]
fn update_empty() {
    let idx = ChmU8::idx_init();
    assert!(!idx.mt_update(10, 20, &cpin()));
}

#[cfg(not(miri))]
const SPAM_QCOUNT: usize = 16_384;
#[cfg(miri)]
const SPAM_QCOUNT: usize = 128;
#[cfg(not(miri))]
const SPAM_TENANTS: usize = 32;
#[cfg(miri)]
const SPAM_TENANTS: usize = 1;

#[derive(Clone, Debug)]
struct ControlToken(Arc<RwLock<()>>);
impl ControlToken {
    fn new() -> Self {
        Self(Arc::new(RwLock::new(())))
    }
    fn acquire_hold(&self) -> RwLockWriteGuard<'_, ()> {
        self.0.write().unwrap()
    }
    fn acquire_permit(&self) -> RwLockReadGuard<'_, ()> {
        self.0.read().unwrap()
    }
}

const TUP_IDENTITY: fn(usize) -> (usize, usize) = |x| (x, x);
const TUP_INCR: fn(usize) -> (usize, usize) = |x| (x, x + 1);
const TUP_INCR_TWICE: fn(usize) -> (usize, usize) = |x| (x, x + 2);

fn prepare_distr_data(source_buf: &[StringTup], distr_buf: &mut Vec<Vec<StringTup>>) {
    distr_buf.try_reserve(SPAM_TENANTS).unwrap();
    distr_buf.extend(source_buf.chunks(SPAM_QCOUNT / SPAM_TENANTS).map(|chunk| {
        chunk
            .iter()
            .map(|(k, v)| (Arc::clone(k), Arc::clone(v)))
            .collect()
    }));
}

fn prepare_data<X, Y, F>(source_buf: &mut Vec<StringTup>, f: F)
where
    F: Fn(usize) -> (X, Y),
    X: ToString,
    Y: ToString,
{
    source_buf.try_reserve(SPAM_QCOUNT).unwrap();
    source_buf.extend(
        (0..SPAM_QCOUNT)
            .into_iter()
            .map(f)
            .map(|(k, v)| (Arc::new(k.to_string()), Arc::new(v.to_string()))),
    );
}

type StringTup = (Arc<String>, Arc<String>);

fn tdistribute_jobs<K, V, F>(
    token: &ControlToken,
    tree: &Arc<ChmCopy<K, V>>,
    distr_data: Vec<Vec<StringTup>>,
    f: F,
) -> Vec<JoinHandle<()>>
where
    F: FnOnce(ControlToken, Arc<ChmCopy<K, V>>, Vec<StringTup>) + Send + 'static + Copy,
    K: Send + Sync + 'static,
    V: Send + Sync + 'static,
{
    let r = distr_data
        .into_iter()
        .enumerate()
        .map(|(tid, this_data)| {
            let this_token = token.clone();
            let this_idx = tree.clone();
            thread::Builder::new()
                .name(tid.to_string())
                .spawn(move || f(this_token, this_idx, this_data))
                .unwrap()
        })
        .collect();
    thread::sleep(Duration::from_millis(1 * SPAM_TENANTS as u64));
    r
}

fn tjoin_all<T>(handles: Vec<JoinHandle<T>>) -> Box<[T]> {
    handles
        .into_iter()
        .map(JoinHandle::join)
        .map(|h| match h {
            Ok(v) => v,
            Err(e) => {
                panic!("thread died with: {:?}", e.downcast_ref::<&str>())
            }
        })
        .collect()
}

fn modify_and_verify_integrity<K, V>(
    token: &ControlToken,
    tree: &Arc<ChmCopy<K, V>>,
    source_buf: &[StringTup],
    action: fn(token: ControlToken, tree: Arc<ChmCopy<K, V>>, thread_chunk: Vec<StringTup>),
    verify: fn(g: &Guard, tree: &ChmCopy<K, V>, src: &[StringTup]),
) where
    K: Send + Sync + 'static,
    V: Send + Sync + 'static,
{
    let mut distr_data = Vec::new();
    prepare_distr_data(source_buf, &mut distr_data);
    let hold = token.acquire_hold();
    let threads = tdistribute_jobs(token, tree, distr_data, action);
    // BLOW THAT INTERCORE TRAFFIC
    drop(hold);
    let _x: Box<[()]> = tjoin_all(threads);
    let pin = cpin();
    verify(&pin, tree, source_buf);
}

fn _action_put(
    token: ControlToken,
    idx: Arc<ChmCopy<Arc<String>, Arc<String>>>,
    data: Vec<StringTup>,
) {
    let _token = token.acquire_permit();
    let g = cpin();
    data.into_iter().for_each(|(k, v)| {
        assert!(idx.mt_insert(k, v, &g));
    });
}
fn _verify_eq(
    pin: &Guard,
    idx: &ChmCopy<Arc<String>, Arc<String>>,
    source: &[(Arc<String>, Arc<String>)],
) {
    assert_eq!(idx.len(), SPAM_QCOUNT);
    source.into_iter().for_each(|(k, v)| {
        assert_eq!(
            idx.mt_get(k, &pin)
                .expect(&format!("failed to find key: {}", k))
                .as_str(),
            v.as_str()
        );
    });
}

#[test]
fn multispam_insert() {
    let idx = Arc::default();
    let token = ControlToken::new();
    let mut data = Vec::new();
    prepare_data(&mut data, TUP_INCR);
    modify_and_verify_integrity(&token, &idx, &data, _action_put, _verify_eq);
}

#[test]
fn multispam_update() {
    let idx = Arc::default();
    let token = ControlToken::new();
    let mut data = Vec::new();
    prepare_data(&mut data, TUP_INCR);
    modify_and_verify_integrity(&token, &idx, &data, _action_put, _verify_eq);
    // update our data set
    data.iter_mut().enumerate().for_each(|(i, (_, v))| {
        *v = Arc::new((i + 2).to_string());
    });
    // store and verify integrity
    modify_and_verify_integrity(
        &token,
        &idx,
        &data,
        |tok, idx, chunk| {
            let g = cpin();
            let _permit = tok.acquire_permit();
            chunk.into_iter().for_each(|(k, v)| {
                let ret = idx
                    .mt_update_return(k.clone(), v, &g)
                    .expect(&format!("couldn't find key: {}", k));
                assert_eq!(
                    ret.as_str().parse::<usize>().unwrap(),
                    (k.parse::<usize>().unwrap() + 1)
                );
            });
            // hmm
        },
        |pin, idx, source| {
            assert_eq!(idx.len(), SPAM_QCOUNT);
            source.into_iter().for_each(|(k, v)| {
                let ret = idx
                    .mt_get(k, &pin)
                    .expect(&format!("couldn't find key: {}", k));
                assert_eq!(ret.as_str(), v.as_str());
            });
        },
    );
}

#[test]
fn multispam_delete() {
    let idx = Arc::default();
    let token = ControlToken::new();
    let mut data = Vec::new();
    prepare_data(&mut data, TUP_INCR);
    modify_and_verify_integrity(&token, &idx, &data, _action_put, _verify_eq);
    // now expunge
    modify_and_verify_integrity(
        &token,
        &idx,
        &data,
        |tok, idx, chunk| {
            let g = cpin();
            let _permit = tok.acquire_permit();
            chunk.into_iter().for_each(|(k, v)| {
                assert_eq!(idx.mt_delete_return(&k, &g).unwrap().as_str(), v.as_str());
            });
        },
        |g, idx, orig| {
            assert!(orig.into_iter().all(|(k, _)| idx.mt_get(k, &g).is_none()));
            assert!(idx.is_empty(), "expected empty, found {} elements instead", idx.len());
        },
    );
}
