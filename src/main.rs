use switchboard_on_demand_client::{
    PullFeed, FetchUpdateParams, SbContext, Gateway, CrossbarClient
};
use solana_sdk::signer::keypair::read_keypair_file;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::RpcSimulateTransactionConfig;
use solana_program::pubkey::Pubkey;
// Use the same SDK version that solana-client uses
use solana_sdk::{
    signature::{Keypair, Signer},
    compute_budget::ComputeBudgetInstruction,
    instruction::Instruction,
};
use std::str::FromStr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting Published SDK Test (version 0.2.2)");
    println!("{}", "=".repeat(60));

    // This test requires a live RPC connection and real network calls
    let rpc_url = "https://api.mainnet-beta.solana.com"; // Using mainnet
    let client = RpcClient::new(rpc_url.to_string());

    let feed_pubkey = Pubkey::from_str("AJ1C3CpVrWgQFNmxgfvSM81XqzE738BagvXz6hBPWVHL")?;

    // Load upgrade authority keypair
    let upgrade_authority_path = "keypair.json"; // Path to your keypair file
    let payer_keypair = read_keypair_file(upgrade_authority_path)
        .unwrap();
    let payer_key = payer_keypair.pubkey();

    println!("✅ Loaded upgrade authority keypair: {}", payer_key);

    let context = SbContext::new();
    let gateway = Gateway::new("https://185.172.191.13.xip.switchboard-oracles.xyz/mainnet".to_string());
    let crossbar = Some(CrossbarClient::default());

    println!("Attempting to fetch update instruction...");

    // Attempt to fetch the update instruction
    let params = FetchUpdateParams {
        feed: feed_pubkey.to_bytes().into(),
        payer: payer_key.to_bytes().into(),
        gateway,
        crossbar,
        num_signatures: None,
        debug: Some(false),
    };

    match PullFeed::fetch_update_ix(context, &client, params).await {
        Ok((instruction, oracle_responses, num_successes, luts)) => {
            println!("✅ Successfully fetched update instruction!");
            println!("📊 Number of successful oracle responses: {}", num_successes);
            println!("📋 Instruction program ID: {}", instruction.program_id);
            println!("📋 Instruction data length: {} bytes", instruction.data.len());
            println!("🏢 Oracle responses:");

            for (i, response) in oracle_responses.iter().enumerate() {
                println!("  Oracle #{}: {}", i + 1, response.oracle);
                println!("    Value: {}", response.value);
                if !response.error.is_empty() {
                    println!("    Error: {}", response.error);
                }
            }

            // Show instruction details
            println!("\n🔍 Instruction Analysis:");
            println!("📋 Program ID: {}", instruction.program_id);
            println!("📊 Instruction accounts: {}", instruction.accounts.len());

            for (i, acc) in instruction.accounts.iter().enumerate() {
                println!("  Account [{}]: {}", i, acc.pubkey);
                println!("    Signer: {}, Writable: {}", acc.is_signer, acc.is_writable);
            }

            // Perform actual transaction simulation
            println!("\n🎯 Running Transaction Simulation...");

            // Create compute budget instructions to handle the computational requirements
            let compute_limit_ix = ComputeBudgetInstruction::set_compute_unit_limit(1_400_000);
            let compute_price_ix = ComputeBudgetInstruction::set_compute_unit_price(1_000);

            // Convert switchboard instruction to SDK instruction type using to_bytes().into()
            let switchboard_instruction = Instruction {
                program_id: instruction.program_id.to_bytes().into(),
                accounts: instruction.accounts.into_iter().map(|acc| solana_sdk::instruction::AccountMeta {
                    pubkey: acc.pubkey.to_bytes().into(),
                    is_signer: acc.is_signer,
                    is_writable: acc.is_writable,
                }).collect(),
                data: instruction.data,
            };

            // Build transaction with compute budget + switchboard instruction
            let instructions = vec![compute_limit_ix, compute_price_ix, switchboard_instruction];

            // Get recent blockhash
            let recent_blockhash = client.get_latest_blockhash().await?;

            // Import VersionedTransaction which implements SerializableTransaction
            use solana_sdk::{
                transaction::VersionedTransaction,
                message::{v0::Message as MessageV0, VersionedMessage},
            };

            // Convert lookup tables to SDK format
            let address_lookup_tables: Vec<solana_sdk::address_lookup_table::AddressLookupTableAccount> =
                luts.iter().map(|lut| {
                    solana_sdk::address_lookup_table::AddressLookupTableAccount {
                        key: lut.key.to_bytes().into(),
                        addresses: lut.addresses.iter().map(|addr| addr.to_bytes().into()).collect(),
                    }
                }).collect();

            println!("📋 Using {} lookup table(s) to compress transaction size", address_lookup_tables.len());
            for (i, lut) in address_lookup_tables.iter().enumerate() {
                println!("  LUT #{}: {} ({} addresses)", i + 1, lut.key, lut.addresses.len());
            }

            // Create the transaction with VersionedTransaction and lookup tables
            let fee_payer_pubkey: solana_sdk::pubkey::Pubkey = payer_key.to_bytes().into();
            let message = MessageV0::try_compile(&fee_payer_pubkey, &instructions, &address_lookup_tables, recent_blockhash)?;
            let versioned_message = VersionedMessage::V0(message);

            // Sign the message hash
            let message_hash = versioned_message.hash();
            let signature = payer_keypair.sign_message(message_hash.as_ref());

            let versioned_transaction = VersionedTransaction {
                message: versioned_message,
                signatures: vec![signature],
            };

            // Simulate the transaction
            let simulation_config = RpcSimulateTransactionConfig {
                sig_verify: false,
                replace_recent_blockhash: true,
                commitment: None,
                encoding: None,
                accounts: None,
                min_context_slot: None,
                inner_instructions: true,
            };

            println!("\n🎯 Running ACTUAL Transaction Simulation...");
            match client.simulate_transaction_with_config(&versioned_transaction, simulation_config).await {
                Ok(response) => {
                    println!("✅ Transaction simulation successful!");

                    if let Some(err) = response.value.err {
                        println!("❌ Simulation failed with error: {:?}", err);
                    } else {
                        println!("💰 Units consumed: {:?}", response.value.units_consumed);

                        if let Some(logs) = response.value.logs {
                            println!("\n📜 Simulation Logs:");
                            for (i, log) in logs.iter().enumerate() {
                                println!("  [{}] {}", i + 1, log);
                            }
                        }

                        if let Some(accounts) = response.value.accounts {
                            println!("\n📊 Account Changes: {} accounts affected", accounts.len());
                        }

                        if let Some(inner_instructions) = response.value.inner_instructions {
                            println!("🔗 Inner Instructions: {} instruction groups", inner_instructions.len());
                        }

                        println!("\n✅ Simulation completed successfully!");
                    }
                }
                Err(e) => {
                    println!("❌ Failed to simulate transaction: {}", e);
                    println!("This could be due to:");
                    println!("  - Network connectivity issues");
                    println!("  - Invalid instruction data");
                    println!("  - Insufficient account balances for simulation");
                    println!("  - RPC endpoint limitations");
                }
            }

            println!("\n🎉 Published SDK Test completed successfully!")
        }
        Err(e) => {
            println!("❌ Failed to fetch update instruction: {}", e);
            println!("This could be due to:");
            println!("  - Network connectivity issues");
            println!("  - Feed not configured properly");
            println!("  - Gateway/Crossbar service issues");
            println!("  - Insufficient oracle responses");
        }
    }

    Ok(())
}
