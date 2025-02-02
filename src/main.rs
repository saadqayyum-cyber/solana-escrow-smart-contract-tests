use std::str::FromStr;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    instruction::{Instruction, AccountMeta},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::Transaction,
};

fn main() {
    // Connect to Solana RPC
    let rpc_url = "http://127.0.0.1:8899";  // Change for testnet/mainnet
    let client = RpcClient::new(rpc_url.to_string());

    // Load your wallet
    let x: &str = "5t6wLX43FSUgYUdyEsyCn5AF4RmhyhH8yUo2k1ctvzPVp2j1E4kQUjD7wwxLRy3tj3TiYrChPYjCUNzuUFv4DtkS";
    let wallet = Keypair::from_base58_string(x);

    // Set your program ID
    let program_id = Pubkey::from_str("DpUanPzBTt89ZWfaotgc1QRJTpGWMSr51uohFktcrsTb")
        .expect("Failed to parse program ID");

    // Create an instruction to call `initialize`
    // Ensure you are using the correct instruction identifier (Anchor provides this identifier).
    let instruction = Instruction::new_with_bincode(
        program_id,
        &(), // You can pass actual data here if needed, but for `initialize`, we pass an empty unit.
        vec![AccountMeta::new(wallet.pubkey(), true)],
    );

    // Build and send the transaction
    let recent_blockhash = client.get_latest_blockhash().unwrap();
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&wallet.pubkey()),
        &[&wallet],
        recent_blockhash,
    );

    let result = client.send_and_confirm_transaction(&transaction);
    match result {
        Ok(sig) => println!("✅ Transaction successful! Signature: {}", sig),
        Err(e) => println!("❌ Transaction failed: {:?}", e),
    }
}
