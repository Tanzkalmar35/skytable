/*
 * Created on Sun Sep 10 2023
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

use crate::engine::config::ConfigAuth;

use {
    crate::engine::error::{Error, QueryResult},
    parking_lot::RwLock,
    std::collections::{hash_map::Entry, HashMap},
};

#[derive(Debug)]
/// The global system configuration
pub struct SysConfig {
    auth_data: Option<RwLock<SysAuth>>,
    host_data: SysHostData,
}

impl PartialEq for SysConfig {
    fn eq(&self, other: &Self) -> bool {
        self.host_data == other.host_data
            && match (self.auth_data.as_ref(), other.auth_data.as_ref()) {
                (None, None) => true,
                (Some(a), Some(b)) => &*a.read() == &*b.read(),
                _ => false,
            }
    }
}

impl SysConfig {
    /// Initialize a new system config
    pub fn new(auth_data: Option<RwLock<SysAuth>>, host_data: SysHostData) -> Self {
        Self {
            auth_data,
            host_data,
        }
    }
    pub fn new_auth(new_auth: Option<ConfigAuth>, host_data: SysHostData) -> Self {
        match new_auth {
            Some(ConfigAuth { root_key, .. }) => Self::new(
                Some(RwLock::new(SysAuth::new(
                    rcrypt::hash(root_key, rcrypt::DEFAULT_COST)
                        .unwrap()
                        .into_boxed_slice(),
                    Default::default(),
                ))),
                host_data,
            ),
            None => Self::new(None, host_data),
        }
    }
    #[cfg(test)]
    /// A test-mode default setting with auth disabled
    pub(super) fn test_default() -> Self {
        Self {
            auth_data: None,
            host_data: SysHostData::new(0, 0),
        }
    }
    /// Returns a handle to the authentication data
    pub fn auth_data(&self) -> &Option<RwLock<SysAuth>> {
        &self.auth_data
    }
    /// Returns a reference to host data
    pub fn host_data(&self) -> &SysHostData {
        &self.host_data
    }
}

#[derive(Debug, PartialEq)]
/// The host data section (system.host)
pub struct SysHostData {
    startup_counter: u64,
    settings_version: u32,
}

impl SysHostData {
    /// New [`SysHostData`]
    pub fn new(startup_counter: u64, settings_version: u32) -> Self {
        Self {
            startup_counter,
            settings_version,
        }
    }
    /// Returns the startup counter
    ///
    /// Note:
    /// - If this is `0` -> this is the first boot
    /// - If this is `1` -> this is the second boot (... and so on)
    pub fn startup_counter(&self) -> u64 {
        self.startup_counter
    }
    /// Returns the settings version
    ///
    /// Note:
    /// - If this is `0` -> this is the initial setting (first boot)
    ///
    /// If it stays at 0, this means that the settings were never changed
    pub fn settings_version(&self) -> u32 {
        self.settings_version
    }
}

/*
    auth
*/

#[derive(Debug, PartialEq)]
/// The auth data section (system.auth)
pub struct SysAuth {
    root_key: Box<[u8]>,
    users: HashMap<Box<str>, SysAuthUser>,
}

impl SysAuth {
    /// New [`SysAuth`] with the given settings
    pub fn new(root_key: Box<[u8]>, users: HashMap<Box<str>, SysAuthUser>) -> Self {
        Self { root_key, users }
    }
    /// Create a new user with the given details
    pub fn create_new_user(&mut self, username: &str, password: &str) -> QueryResult<()> {
        match self.users.entry(username.into()) {
            Entry::Vacant(ve) => {
                ve.insert(SysAuthUser::new(
                    rcrypt::hash(password, rcrypt::DEFAULT_COST)
                        .unwrap()
                        .into_boxed_slice(),
                ));
                Ok(())
            }
            Entry::Occupied(_) => Err(Error::SysAuthError),
        }
    }
    /// Verify the user with the given details
    pub fn verify_user(&self, username: &str, password: &str) -> QueryResult<()> {
        if username == "root" {
            if rcrypt::verify(password, self.root_key()).unwrap() {
                return Ok(());
            } else {
                return Err(Error::SysAuthError);
            }
        }
        match self.users.get(username) {
            Some(user) if rcrypt::verify(password, user.key()).unwrap() => Ok(()),
            Some(_) | None => Err(Error::SysAuthError),
        }
    }
    pub fn root_key(&self) -> &[u8] {
        &self.root_key
    }
    pub fn users(&self) -> &HashMap<Box<str>, SysAuthUser> {
        &self.users
    }
}

#[derive(Debug, PartialEq)]
/// The auth user
pub struct SysAuthUser {
    key: Box<[u8]>,
}

impl SysAuthUser {
    /// Create a new [`SysAuthUser`]
    pub fn new(key: Box<[u8]>) -> Self {
        Self { key }
    }
    /// Get the key
    pub fn key(&self) -> &[u8] {
        self.key.as_ref()
    }
}
