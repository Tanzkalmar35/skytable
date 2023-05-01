/*
 * Created on Sat Jan 28 2023
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

#[cfg(debug_assertions)]
use super::CHTRuntimeLog;
use {
    super::{
        iter::{IterKV, IterKey, IterVal},
        meta::{Config, TreeElement},
        patch::{VanillaInsert, VanillaUpdate, VanillaUpdateRet, VanillaUpsert},
        RawTree,
    },
    crate::engine::{
        idx::{meta::Comparable, AsKeyClone, AsValue, AsValueClone, IndexBaseSpec, MTIndex},
        sync::atm::Guard,
    },
    std::sync::Arc,
};

pub type Raw<E, C> = RawTree<E, C>;
pub type ChmArc<K, V, C> = Raw<Arc<(K, V)>, C>;
pub type ChmCopy<K, V, C> = Raw<(K, V), C>;

impl<E, C: Config> IndexBaseSpec for Raw<E, C> {
    const PREALLOC: bool = false;

    type Metrics = CHTRuntimeLog;

    fn idx_init() -> Self {
        Self::new()
    }

    fn idx_init_with(s: Self) -> Self {
        s
    }

    fn idx_metrics(&self) -> &Self::Metrics {
        &self.m
    }
}

impl<E: TreeElement, C: Config> MTIndex<E::Key, E::Value> for Raw<E, C> {
    type IterKV<'t, 'g, 'v> = IterKV<'t, 'g, 'v, E, C>
    where
        'g: 't + 'v,
        't: 'v,
        E::Key: 'v,
        E::Value: 'v,
        Self: 't;

    type IterKey<'t, 'g, 'v> = IterKey<'t, 'g, 'v, E, C>
    where
        'g: 't + 'v,
        't: 'v,
        E::Key: 'v,
        Self: 't;

    type IterVal<'t, 'g, 'v> = IterVal<'t, 'g, 'v, E, C>
    where
        'g: 't + 'v,
        't: 'v,
        E::Value: 'v,
        Self: 't;

    fn mt_clear(&self, g: &Guard) {
        self.nontransactional_clear(g)
    }

    fn mt_insert(&self, key: E::Key, val: E::Value, g: &Guard) -> bool
    where
        E::Value: AsValue,
    {
        self.patch(VanillaInsert(E::new(key, val)), g)
    }

    fn mt_upsert(&self, key: E::Key, val: E::Value, g: &Guard)
    where
        E::Value: AsValue,
    {
        self.patch(VanillaUpsert(E::new(key, val)), g)
    }

    fn mt_contains<Q>(&self, key: &Q, g: &Guard) -> bool
    where
        Q: ?Sized + Comparable<E::Key>,
    {
        self.contains_key(key, g)
    }

    fn mt_get<'t, 'g, 'v, Q>(&'t self, key: &Q, g: &'g Guard) -> Option<&'v E::Value>
    where
        Q: ?Sized + Comparable<E::Key>,
        't: 'v,
        'g: 't + 'v,
    {
        self.get(key, g)
    }

    fn mt_get_cloned<Q>(&self, key: &Q, g: &Guard) -> Option<E::Value>
    where
        Q: ?Sized + Comparable<E::Key>,
        E::Value: AsValueClone,
    {
        self.get(key, g).cloned()
    }

    fn mt_update(&self, key: E::Key, val: E::Value, g: &Guard) -> bool
    where
        E::Key: AsKeyClone,
        E::Value: AsValue,
    {
        self.patch(VanillaUpdate(E::new(key, val)), g)
    }

    fn mt_update_return<'t, 'g, 'v>(
        &'t self,
        key: E::Key,
        val: E::Value,
        g: &'g Guard,
    ) -> Option<&'v E::Value>
    where
        E::Key: AsKeyClone,
        E::Value: AsValue,
        't: 'v,
        'g: 't + 'v,
    {
        self.patch(VanillaUpdateRet(E::new(key, val)), g)
    }

    fn mt_delete<Q>(&self, key: &Q, g: &Guard) -> bool
    where
        Q: ?Sized + Comparable<E::Key>,
    {
        self.remove(key, g)
    }

    fn mt_delete_return<'t, 'g, 'v, Q>(&'t self, key: &Q, g: &'g Guard) -> Option<&'v E::Value>
    where
        Q: ?Sized + Comparable<E::Key>,
        't: 'v,
        'g: 't + 'v,
    {
        self.remove_return(key, g)
    }
}
