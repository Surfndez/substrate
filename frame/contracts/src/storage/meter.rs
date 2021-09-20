// This file is part of Substrate.

// Copyright (C) 2019-2021 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! This module contains functions to meter the storage usage.

use crate::{BalanceOf, Config, Error};
use frame_support::{dispatch::{DispatchError, DispatchResult}, DefaultNoBound};
use sp_core::crypto::UncheckedFrom;
use sp_runtime::traits::Saturating;
use sp_std::{marker::PhantomData, mem};

pub trait Ext<T: Config> {
	fn reserve_limit(origin: &T::AccountId, limit: &BalanceOf<T>) -> DispatchResult;
	fn unreserve_limit(origin: &T::AccountId, remaining: &BalanceOf<T>);
	fn charge(origin: &T::AccountId, contract: &T::AccountId, amount: &Cost<T>) -> DispatchResult;
}

pub struct RootMeter<'a, T: Config, E> {
	origin: &'a T::AccountId,
	limit: BalanceOf<T>,
	usage: Usage<T>,
	_ext: PhantomData<E>,
}

pub struct NestedMeter<'root, T: Config, E> {
	root: &'root mut RootMeter<'root, T, E>,
	contract: T::AccountId,
	usage: Usage<T>,
}

pub enum Cost<T: Config> {
	Charge(BalanceOf<T>),
	Refund(BalanceOf<T>),
}

#[derive(DefaultNoBound, Clone)]
pub struct Usage<T: Config> {
	charged: BalanceOf<T>,
	refunded: BalanceOf<T>,
}

impl<T: Config> Copy for Usage<T> {}

impl<T: Config> Usage<T> {
	fn cost(&self) -> Cost<T> {
		if self.charged >= self.refunded {
			Cost::Charge(self.charged.saturating_sub(self.refunded))
		} else {
			Cost::Refund(self.refunded.saturating_sub(self.charged))
		}
	}
}

impl<T: Config> Saturating for Usage<T> {
	fn saturating_add(self, rhs: Self) -> Self {
		Self {
			charged: self.charged.saturating_add(rhs.charged),
			refunded: self.refunded.saturating_add(rhs.refunded),
		}
	}

	fn saturating_sub(self, rhs: Self) -> Self {
		Self {
			charged: self.charged.saturating_sub(rhs.charged),
			refunded: self.refunded.saturating_sub(rhs.refunded),
		}
	}

	fn saturating_mul(self, rhs: Self) -> Self {
		Self {
			charged: self.charged.saturating_mul(rhs.charged),
			refunded: self.refunded.saturating_mul(rhs.refunded),
		}
	}

	fn saturating_pow(self, exp: usize) -> Self {
		Self {
			charged: self.charged.saturating_pow(exp),
			refunded: self.refunded.saturating_pow(exp),
		}
	}
}

impl<'a, T, E> RootMeter<'a, T, E>
where
	T: Config,
	T::AccountId: UncheckedFrom<T::Hash> + AsRef<[u8]>,
	E: Ext<T>,
{
	pub fn new(
		origin: &'a T::AccountId,
		limit: BalanceOf<T>,
	) -> Result<Self, DispatchError> {
		E::reserve_limit(&origin, &limit)?;
		Ok(Self {
			origin,
			limit,
			usage: Default::default(),
			_ext: PhantomData,
		})
	}

	pub fn nested(&'a mut self, contract: T::AccountId) -> NestedMeter<T, E> {
		NestedMeter {
			root: self,
			contract,
			usage: Default::default(),
		}
	}
}

impl<'root, T, E> NestedMeter<'root, T, E>
where
	T: Config,
	T::AccountId: UncheckedFrom<T::Hash> + AsRef<[u8]>,
	E: Ext<T>,
{
	pub fn absorb(self, persist: bool) {
		if !persist {
			// revert changes to the overall usage
			self.root.usage = self.root.usage.saturating_sub(self.usage);
		} else {
			// TODO: calc what needs to be charged refunded
		}
	}

	pub fn charge(&mut self, usage: Usage<T>) -> DispatchResult {
		self.root.usage = self.root.usage.saturating_add(usage);
		self.usage = self.usage.saturating_add(usage);
		if let Cost::Charge(amount) = self.root.usage.cost() {
			if amount > self.root.limit {
				return Err(<Error<T>>::StorageExhausted.into());
			}
		}
		E::charge(self.root.origin, &self.contract, &self.usage.cost())
	}
}
