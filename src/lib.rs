#![cfg_attr(not(any(test, feature = "export-abi")), no_main)]
#![cfg_attr(not(any(test, feature = "export-abi")), no_std)]

#[macro_use]
extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
// use alloy_sol_types::sol_data::String;
use alloy_primitives::aliases::U56;
use stylus_sdk::{
    alloy_primitives::{Address, U256},
    alloy_sol_types::{sol, SolError},
    block,
    call::transfer_eth,
    contract, evm, msg,
    prelude::*,
    storage::{StorageAddress, StorageBool, StorageMap, StorageString, StorageU256, StorageVec},
};
// use stylus_sdk::context;

sol! {
    error InsufficientBalance();
    error InsufficientAllowance();
    error ZeroAddress();
    error Paused();
    error InvalidAmount();
    error WithdrawalDelayNotMet();
    error AlreadyClaimed();
    error NotYourRequest();
    error InsufficientContractBalance();
    error TransferFailed();

    error Unauthorized();
}
sol! {
    event Transfer(address indexed from, address indexed to, uint256 value);
    event Approval(address indexed owner, address indexed spender, uint256 value);
    event Staked(address indexed user, uint256 ethAmount, uint256 stEthMinted);
    event Unstaked(address indexed user, uint256 stEthBurned, uint256 ethReturned);
    event RewardsDistributed(uint256 totalRewards);
    event WithdrawalRequested(address indexed user, uint256 stEthAmount, uint256 requestId);
    event WithdrawalClaimed(address indexed user, uint256 requestId, uint256 ethAmount);
    event OwnershipTransferred(address indexed previousOwner, address indexed newOwner);
    event Pause();
    event Unpaused();
}

sol_storage! {
    pub struct WithdrawalRequest{
        StorageAddress user;
        StorageU256 st_eth_amount;
        StorageU256 request_time;
        StorageBool claimed;
    }
}

sol_storage! {
    #[entrypoint]
    pub struct LiquidStaking{
        // ERC20 token data
        StorageString name;
        StorageString symbol;
        StorageU256 decimals;
        StorageU256 total_supply;
        StorageMap<Address,StorageU256> balances;
        StorageMap<Address,StorageMap<Address,StorageU256>> allowances;
        // Staking specific data
        StorageU256 total_staked_eth;
        StorageU256 rewards_accumulated;
        StorageU256 withdrawal_delay;
        StorageU256 request_counter;
        StorageU256 apy;
        StorageU256 last_reward_update;
        // Access Control
        StorageAddress owner;
        StorageBool paused;

        // Withdrawal requests mapping
        StorageMap<U256,WithdrawalRequest> withdrawal_requests;

        StorageMap<Address , StorageVec<StorageU256>> user_withdrawal_requests;
    }

}

const BASIS_POINTS: U256 = U256::from_limbs([10000, 0, 0, 0]);
const SECONDS_PER_YEAR: U256 = U256::from_limbs([31536000, 0, 0, 0]);
const SEVEN_DAYS: U256 = U256::from_limbs([604800, 0, 0, 0]);
const THIRTY_DAYS: U256 = U256::from_limbs([2592000, 0, 0, 0]);
const MAX_APY: U256 = U256::from_limbs([2000, 0, 0, 0]);
const ONE_ETHER: U256 = U256::from_limbs([1000000000000000000, 0, 0, 0]);

#[public]
impl LiquidStaking {
    // constructor - initialize the contract
    pub fn initialize(&mut self) -> Result<(), Vec<u8>> {
        let sender = msg::sender();

        // Initialize ERC20 token data
        self.name.set_str("Staked Ether");
        self.symbol.set_str("stEth");
        self.decimals.set(U256::from(18));
        self.total_supply.set(U256::ZERO);

        // Initialize staking data
        self.total_staked_eth.set(U256::ZERO);
        self.rewards_accumulated.set(U256::ZERO);
        self.withdrawal_delay.set(SEVEN_DAYS);
        self.request_counter.set(U256::ZERO);
        self.apy.set(U256::from(500));
        self.last_reward_update.set(U256::from(block::timestamp()));

        self.owner.set(sender);
        self.paused.set(false);

        evm::log(OwnershipTransferred {
            previousOwner: Address::ZERO,
            newOwner: sender,
        });
        Ok(())
    }

    fn only_owner(&self) -> Result<(), Vec<u8>> {
        if msg::sender() != self.owner.get() {
            return Err(Unauthorized {}.abi_encode());
        }
        Ok(())
    }

    fn when_not_paused(&self) -> Result<(), Vec<u8>> {
        if self.paused.get() {
            return Err(Paused {}.abi_encode());
        }
        Ok(())
    }

    pub fn name(&self) -> Result<String, Vec<u8>> {
        Ok(self.name.get_string())
    }

    pub fn symbol(&self) -> Result<String, Vec<u8>> {
        Ok(self.symbol.get_string())
    }

    pub fn decimals(&self) -> Result<U256, Vec<u8>> {
        Ok(self.decimals.get())
    }

    pub fn totalSupply(&self) -> Result<U256, Vec<u8>> {
        Ok(self.total_supply.get())
    }

    pub fn balance_of(&self, account: Address) -> Result<U256, Vec<u8>> {
        Ok(self.balances.get(account))
    }

    pub fn transfer(&mut self, to: Address, amount: U256) -> Result<bool, Vec<u8>> {
        let from = msg::sender();
        self._transfer(from, to, amount)?;
        Ok(true)
    }

    pub fn allowance(&self, owner: Address, spender: Address) -> Result<U256, Vec<u8>> {
        Ok(self.allowances.get(owner).get(spender))
    }

    pub fn approve(&mut self, spender: Address, amount: U256) -> Result<bool, Vec<u8>> {
        let owner = msg::sender();
        self._approve(owner, spender, amount)?;
        Ok(true)
    }

    pub fn transfer_from(
        &mut self,
        from: Address,
        to: Address,
        amount: U256,
    ) -> Result<bool, Vec<u8>> {
        let spender = msg::sender();
        let current_allowance = self.allowances.get(from).get(spender);

        if current_allowance < amount {
            return Err(InsufficientAllowance {}.abi_encode());
        }
        self._transfer(from, to, amount)?;
        self._approve(from, spender, current_allowance - amount)?;
        Ok(true)
    }
    #[payable]
    pub fn stake(&mut self) -> Result<(), Vec<u8>> {
        self.when_not_paused()?;

        let eth_amount = msg::value();
        if eth_amount == U256::ZERO {
            return Err(InvalidAmount {}.abi_encode());
        }

        self.update_rewards()?;

        let st_eth_to_mint = if self.total_supply.get() == U256::ZERO {
            eth_amount
        } else {
            // Calculate stETH to mint based on current exchange rate
            (eth_amount * self.total_supply.get()) / self.total_staked_eth.get()
        };
        let sender = msg::sender();
        self._mint(sender, st_eth_to_mint)?;

        evm::log(Staked {
            user: sender,
            ethAmount: eth_amount,
            stEthMinted: st_eth_to_mint,
        });
        Ok(())
    }

    pub fn request_withdrawal(&mut self, st_eth_amount: U256) -> Result<(), Vec<u8>> {
        self.when_not_paused()?;

        let sender = msg::sender();
        let balance = self.balances.get(sender);
        if balance < st_eth_amount {
            return Err(InsufficientBalance {}.abi_encode());
        }
        self.update_rewards()?;

        self._burn(sender, st_eth_amount)?;

        let request_id = self.request_counter.get();
        self.request_counter.set(request_id + U256::from(1));

        let mut request = self.withdrawal_requests.setter(request_id);
        request.user.set(sender);

        request.st_eth_amount.set(st_eth_amount);
        request.request_time.set(U256::from(block::timestamp()));

        request.claimed.set(false);

        self.user_withdrawal_requests
            .setter(sender)
            .push(request_id.into());

        evm::log(WithdrawalRequested {
            user: sender,
            stEthAmount: st_eth_amount,
            requestId: request_id,
        });
        Ok(())
    }

    pub fn claim_withdrawal(&mut self, request_id: U256) -> Result<(), Vec<u8>> {
        let sender = msg::sender();
        let request = self.withdrawal_requests.get(request_id);

        if request.user.get() != sender {
            return Err(NotYourRequest {}.abi_encode());
        }

        if request.claimed.get() {
            return Err(AlreadyClaimed {}.abi_encode());
        }

        let current_time = U256::from(block::timestamp());
        let required_time = request.request_time.get() + self.withdrawal_delay.get();
        if current_time < required_time {
            return Err(WithdrawalDelayNotMet {}.abi_encode());
        }

        // Calculate ETH to return based on current exchange rate
        let eth_to_return = if self.total_supply.get() == U256::ZERO {
            request.st_eth_amount.get()
        } else {
            (request.st_eth_amount.get() * self.total_staked_eth.get()) / self.total_supply.get()
        };

        // Check contract balance
        if contract::balance() < eth_to_return {
            return Err(InsufficientContractBalance {}.abi_encode());
        }

        // Mark as claimed
        self.withdrawal_requests
            .setter(request_id)
            .claimed
            .set(true);

        // Update total staked ETH
        let total_staked = self.total_staked_eth.get();
        self.total_staked_eth.set(total_staked - eth_to_return);

        // Transfer ETH to user
        transfer_eth(sender, eth_to_return)?;

        evm::log(WithdrawalClaimed {
            user: sender,
            requestId: request_id,
            ethAmount: eth_to_return,
        });

        Ok(())
    }

    pub fn get_exchange_rate(&self) -> Result<U256, Vec<u8>> {
        if self.total_supply.get() == U256::ZERO {
            Ok(ONE_ETHER) // 1:1 ratio initially
        } else {
            Ok((self.total_staked_eth.get() * ONE_ETHER) / self.total_supply.get())
        }
    }

    pub fn st_eth_to_eth(&self, st_eth_amount: U256) -> Result<U256, Vec<u8>> {
        if self.total_supply.get() == U256::ZERO {
            Ok(st_eth_amount)
        } else {
            Ok((st_eth_amount * self.total_staked_eth.get()) / self.total_supply.get())
        }
    }

    pub fn eth_to_st_eth(&self, eth_amount: U256) -> Result<U256, Vec<u8>> {
        if self.total_supply.get() == U256::ZERO {
            Ok(eth_amount)
        } else {
            Ok((eth_amount * self.total_supply.get()) / self.total_staked_eth.get())
        }
    }

    pub fn update_rewards(&mut self) -> Result<(), Vec<u8>> {
        if self.total_staked_eth.get() == U256::ZERO {
            self.last_reward_update.set(U256::from(block::timestamp()));
            return Ok(());
        }
        let current_time = U256::from(block::timestamp());
        let last_update = self.last_reward_update.get();
        if current_time <= last_update {
            return Ok(());
        }
        let time_elapsed = current_time - last_update;

        // Calculate rewards: (totalStaked * APY * timeElapsed) / (365 days * BASIS_POINTS)
        let rewards = (self.total_staked_eth.get() * self.apy.get() * time_elapsed)
            / (SECONDS_PER_YEAR * BASIS_POINTS);

        if rewards > U256::ZERO {
            let total_staked = self.total_staked_eth.get();
            self.total_staked_eth.set(total_staked + rewards);

            let accumulated = self.rewards_accumulated.get();
            self.rewards_accumulated.set(accumulated + rewards);

            self.last_reward_update.set(current_time);

            evm::log(RewardsDistributed {
                totalRewards: rewards,
            });
        }

        Ok(())
    }

    pub fn get_user_withdrawal_requests(&self, user: Address) -> Result<Vec<U256>, Vec<u8>> {
        let requests = self.user_withdrawal_requests.get(user);
        let mut result = Vec::new();
        for i in 0..requests.len() {
            if let Some(request_id) = requests.get(i) {
                result.push(request_id);
            }
        }
        Ok(result)
    }

    pub fn can_claim_withdrawal(&self, request_id: U256) -> Result<bool, Vec<u8>> {
        let request = self.withdrawal_requests.get(request_id);
        let current_time = U256::from(block::timestamp());
        let required_time = request.request_time.get() + self.withdrawal_delay.get();

        Ok(!request.claimed.get() && current_time >= required_time)
    }

    pub fn get_withdrawal_request(
        &self,
        request_id: U256,
    ) -> Result<(Address, U256, U256, bool), Vec<u8>> {
        let request = self.withdrawal_requests.get(request_id);
        Ok((
            request.user.get(),
            request.st_eth_amount.get(),
            request.request_time.get(),
            request.claimed.get(),
        ))
    }

    // Getter functions for public variables
    pub fn total_staked_eth(&self) -> Result<U256, Vec<u8>> {
        Ok(self.total_staked_eth.get())
    }

    pub fn rewards_accumulated(&self) -> Result<U256, Vec<u8>> {
        Ok(self.rewards_accumulated.get())
    }

    pub fn withdrawal_delay(&self) -> Result<U256, Vec<u8>> {
        Ok(self.withdrawal_delay.get())
    }

    pub fn apy(&self) -> Result<U256, Vec<u8>> {
        Ok(self.apy.get())
    }

    pub fn last_reward_update(&self) -> Result<U256, Vec<u8>> {
        Ok(self.last_reward_update.get())
    }

    pub fn owner(&self) -> Result<Address, Vec<u8>> {
        Ok(self.owner.get())
    }

    pub fn paused(&self) -> Result<bool, Vec<u8>> {
        Ok(self.paused.get())
    }

    // Admin Functions
    pub fn set_apy(&mut self, new_apy: U256) -> Result<(), Vec<u8>> {
        self.only_owner()?;

        if new_apy > MAX_APY {
            return Err(InvalidAmount {}.abi_encode());
        }

        self.update_rewards()?;
        self.apy.set(new_apy);
        Ok(())
    }

    pub fn set_withdrawal_delay(&mut self, new_delay: U256) -> Result<(), Vec<u8>> {
        self.only_owner()?;
        if new_delay > THIRTY_DAYS {
            return Err(InvalidAmount {}.abi_encode());
        }
        self.withdrawal_delay.set(new_delay);
        Ok(())
    }

    #[payable]
    pub fn add_rewards(&mut self) -> Result<(), Vec<u8>> {
        self.only_owner()?;

        let reward_amount = msg::value();
        if reward_amount == U256::ZERO {
            return Err(InvalidAmount {}.abi_encode());
        }
        let total_staked = self.total_staked_eth.get();
        self.total_staked_eth.set(total_staked + reward_amount);

        let accumulated = self.rewards_accumulated.get();
        self.rewards_accumulated.set(accumulated + reward_amount);
        evm::log(RewardsDistributed {
            totalRewards: reward_amount,
        });
        Ok(())
    }

    pub fn pause(&mut self) -> Result<(), Vec<u8>> {
        self.only_owner()?;
        self.paused.set(true);
        evm::log(Pause {});
        Ok(())
    }

    pub fn unpause(&mut self) -> Result<(), Vec<u8>> {
        self.only_owner()?;
        self.paused.set(false);
        evm::log(Unpaused {});
        Ok(())
    }

    pub fn transfer_ownership(&mut self, new_owner: Address) -> Result<(), Vec<u8>> {
        self.only_owner()?;

        if new_owner == Address::ZERO {
            return Err(ZeroAddress {}.abi_encode());
        }

        let old_owner = self.owner.get();
        self.owner.set(new_owner);

        evm::log(OwnershipTransferred {
            previousOwner: old_owner,
            newOwner: new_owner,
        });

        Ok(())
    }

    pub fn emergency_withdraw(&mut self, amount: U256) -> Result<(), Vec<u8>> {
        self.only_owner()?;

        if !self.paused.get() {
            return Err(Paused {}.abi_encode());
        }

        let contract_balance = contract::balance();
        if amount > contract_balance {
            return Err(InsufficientContractBalance {}.abi_encode());
        }

        let owner = self.owner.get();
        transfer_eth(owner, amount)?;

        Ok(())
    }

    #[receive]
    pub fn receive(&mut self) -> Result<(), Vec<u8>> {
        let sender = msg::sender();
        if sender != self.owner.get() {
            return Err(Unauthorized {}.abi_encode());
        }
        Ok(())
    }
}

impl LiquidStaking {
    fn _burn(&mut self, from: Address, amount: U256) -> Result<(), Vec<u8>> {
        if from == Address::ZERO {
            return Err(ZeroAddress {}.abi_encode());
        }
        let balance = self.balances.get(from);
        if balance < amount {
            return Err(InsufficientBalance {}.abi_encode());
        }
        self.balances.setter(from).set(balance - amount);
        let total_supply = self.total_supply.get();
        self.total_supply.set(total_supply - amount);
        evm::log(Transfer {
            from,
            to: Address::ZERO,
            value: amount,
        });
        Ok(())
    }

    fn _approve(&mut self, owner: Address, spender: Address, amount: U256) -> Result<(), Vec<u8>> {
        if owner == Address::ZERO || spender == Address::ZERO {
            return Err(ZeroAddress {}.abi_encode());
        }
        self.allowances.setter(owner).setter(spender).set(amount);
        evm::log(Approval {
            owner,
            spender,
            value: amount,
        });
        Ok(())
    }

    fn _mint(&mut self, to: Address, amount: U256) -> Result<(), Vec<u8>> {
        if to == Address::ZERO {
            return Err(ZeroAddress {}.abi_encode());
        }
        let total_supply = self.total_supply.get();
        self.total_supply.set(total_supply + amount);

        let balance = self.balances.get(to);
        self.balances.setter(to).set(balance + amount);
        evm::log(Transfer {
            from: Address::ZERO,
            to,
            value: amount,
        });
        Ok(())
    }

    fn _transfer(&mut self, from: Address, to: Address, amount: U256) -> Result<(), Vec<u8>> {
        if from == Address::ZERO || to == Address::ZERO {
            return Err(ZeroAddress {}.abi_encode());
        }

        let from_balance = self.balances.get(from);
        if from_balance < amount {
            return Err(InsufficientBalance {}.abi_encode());
        }
        self.balances.setter(from).set(from_balance - amount);
        let to_balance = self.balances.get(to);
        self.balances.setter(to).set(to_balance + amount);
        evm::log(Transfer {
            from,
            to,
            value: amount,
        });
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use alloy_primitives::{address, Address, U256};
    use motsu::prelude::*;
    use LiquidStaking;

    const ALICE: Address = address!("0000000000000000000000000000000000000001");
    const BOB: Address = address!("0000000000000000000000000000000000000002");
    const CAROL: Address = address!("0000000000000000000000000000000000000003");
    const OWNER: Address = address!("1000000000000000000000000000000000000001");
    const ONE_ETHER: U256 = U256::from_limbs([1000000000000000000, 0, 0, 0]);
    const SEVEN_DAYS: u64 = 7 * 24 * 60 * 60;

    #[motsu::test]
    fn test_contract_initialize(contract: Contract<LiquidStaking>) {
        let result = contract.sender(ALICE).initialize();
        assert!(result.is_ok());
    }

    #[motsu::test]
    fn test_contract_approve(contract: Contract<LiquidStaking>) {
        let result = contract.sender(ALICE).approve(BOB, ONE_ETHER);
        assert!(result.is_ok());
    }
}
