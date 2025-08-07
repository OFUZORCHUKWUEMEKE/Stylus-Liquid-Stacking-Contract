# 🌊 Liquid Staking Contract

A decentralized liquid staking protocol built with **Arbitrum Stylus** and **Rust**, allowing users to stake ETH and receive liquid stETH tokens that accrue value over time.

![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=for-the-badge&logo=rust&logoColor=white)
![Ethereum](https://img.shields.io/badge/Ethereum-3C3C3D?style=for-the-badge&logo=Ethereum&logoColor=white)
![Arbitrum](https://img.shields.io/badge/Arbitrum-2D374B?style=for-the-badge&logo=arbitrum&logoColor=white)

## ✨ Features

- 🔄 **Liquid Staking**: Stake ETH and receive tradeable stETH tokens
- 📈 **Auto-Compounding**: Rewards automatically increase stETH value
- ⏱️ **Withdrawal Delays**: 7-day security delay for withdrawals
- 🛡️ **Security**: Built with Rust's memory safety and formal verification
- ⚡ **Low Gas**: 50%+ gas savings compared to Solidity contracts
- 🎯 **ERC20 Compatible**: stETH works with all DeFi protocols

### Installation

```bash
# Clone the repository
git clone https://github.com/yourusername/liquid-staking-stylus.git
cd liquid-staking-stylus

# Install Cargo Stylus
cargo install --force cargo-stylus

# Install dependencies
cargo build
```

### Deploy to Arbitrum Sepolia

```bash
# Set your private key
export PRIVATE_KEY="your_private_key_here"

# Deploy the contract
cargo stylus deploy \
  --private-key-path=.secret \
  --endpoint="https://sepolia-rollup.arbitrum.io/rpc"
```

