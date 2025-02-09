use anchor_lang::{
    account, solana_program::hash::hash, system_program::ID, AccountDeserialize, AnchorDeserialize,
    AnchorSerialize,
};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_program,
    transaction::Transaction,
};
use std::str::FromStr;

// Constants
const RPC_URL: &str = "http://localhost:8899";
const PROGRAM_ID: &str = "ABkdGF6rfAVxU9zC9n961YBTLKmNAEM3waZ2936fa1f";
const LAMPORTS_PER_SOL: u64 = 1_000_000_000;
const DEFAULT_VALIDATION_THRESHOLD: u64 = 1000;
const BUYER_INITIAL_BALANCE: u64 = 10 * LAMPORTS_PER_SOL;
const SELLER_INITIAL_BALANCE: u64 = 1 * LAMPORTS_PER_SOL;

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct StartSubscriptionArgs {
    pub subscription_id: String,
    pub validation_threshold: u64,
}

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct MakePaymentArgs {
    pub amount: u64,
}

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct WithdrawFundsArgs {
    pub validation_data: u64,
}

#[account]
#[derive(Default)]
pub struct EscrowAccount {
    pub seller: Pubkey,
    pub buyer: Pubkey,
    pub subscription_id: String,
    pub payment_count: u8,
    pub total_amount: u64,
    pub is_active: bool,
    pub validation_threshold: u64,
}

fn get_instruction_sighash(name: &str) -> [u8; 8] {
    let preimage = format!("global:{}", name);
    let hash = hash(preimage.as_bytes());
    let mut sighash = [0u8; 8];
    sighash.copy_from_slice(&hash.to_bytes()[..8]);
    sighash
}

struct TestContext {
    client: RpcClient,
    program_id: Pubkey,
    buyer: Keypair,
    seller: Keypair,
}

#[derive(Debug)]
struct Balance {
    seller: u64,
    escrow: u64,
    buyer: u64,
}

impl TestContext {
    fn new() -> Self {
        let rpc_url = RPC_URL;
        let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());
        let program_id = Pubkey::from_str(PROGRAM_ID).expect("Failed to parse program ID");

        let buyer = Keypair::new();
        let seller = Keypair::new();

        Self {
            client,
            program_id,
            buyer,
            seller,
        }
    }

    fn find_subscription_pda(&self, subscription_id: &str) -> (Pubkey, u8) {
        Pubkey::find_program_address(
            &[
                b"escrow",
                self.buyer.pubkey().as_ref(),
                self.seller.pubkey().as_ref(),
                subscription_id.as_bytes(),
            ],
            &self.program_id,
        )
    }

    async fn get_balances(
        &self,
        subscription_pda: &Pubkey,
        label: &str,
        log: bool,
    ) -> Result<Balance, Box<dyn std::error::Error>> {
        let seller_balance = self.client.get_balance(&self.seller.pubkey())?;
        let buyer_balance = self.client.get_balance(&self.buyer.pubkey())?;
        let escrow_balance = self.client.get_balance(subscription_pda)?;

        if log {
            println!("\n=== Balances at {} ===", label);
            println!(
                "Seller: {} SOL",
                seller_balance as f64 / LAMPORTS_PER_SOL as f64
            );
            println!(
                "Escrow: {} SOL",
                escrow_balance as f64 / LAMPORTS_PER_SOL as f64
            );
            println!(
                "Buyer: {} SOL",
                buyer_balance as f64 / LAMPORTS_PER_SOL as f64
            );
            println!("========================\n");
        }

        Ok(Balance {
            seller: seller_balance,
            escrow: escrow_balance,
            buyer: buyer_balance,
        })
    }

    async fn request_airdrop_with_confirmation(
        &self,
        pubkey: &Pubkey,
        amount: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for attempt in 0..3 {
            println!("Airdrop attempt {} for {}", attempt + 1, pubkey);

            match self.client.request_airdrop(pubkey, amount) {
                Ok(signature) => {
                    // Wait for confirmation
                    for _ in 0..32 {
                        if self.client.confirm_transaction(&signature)? {
                            // Verify the balance after confirmation
                            let balance = self.get_balance(pubkey)?;
                            if balance >= amount {
                                println!(
                                    "✅ Airdrop confirmed. Balance: {} SOL",
                                    balance as f64 / LAMPORTS_PER_SOL as f64
                                );
                                return Ok(());
                            }
                        }
                        std::thread::sleep(std::time::Duration::from_millis(500));
                    }
                }
                Err(e) => {
                    println!("Airdrop request failed: {}", e);
                }
            }

            // Wait before retry
            std::thread::sleep(std::time::Duration::from_secs(1));
        }

        Err("Failed to complete airdrop after multiple attempts".into())
    }

    async fn setup(&self) -> Result<(), Box<dyn std::error::Error>> {
        println!("Setting up test accounts...");

        // Fund buyer
        println!("\nFunding buyer account...");
        self.request_airdrop_with_confirmation(&self.buyer.pubkey(), BUYER_INITIAL_BALANCE)
            .await?;

        // Fund seller
        println!("\nFunding seller account...");
        self.request_airdrop_with_confirmation(&self.seller.pubkey(), SELLER_INITIAL_BALANCE)
            .await?;

        // Final balance verification
        let buyer_balance = self.get_balance(&self.buyer.pubkey())?;
        let seller_balance = self.get_balance(&self.seller.pubkey())?;

        println!("\nFinal balances:");
        println!(
            "Buyer: {} SOL",
            buyer_balance as f64 / LAMPORTS_PER_SOL as f64
        );
        println!(
            "Seller: {} SOL",
            seller_balance as f64 / LAMPORTS_PER_SOL as f64
        );

        if buyer_balance < LAMPORTS_PER_SOL || seller_balance < LAMPORTS_PER_SOL {
            return Err("Failed to fund accounts with sufficient SOL".into());
        }

        Ok(())
    }

    fn get_balance(&self, pubkey: &Pubkey) -> Result<u64, Box<dyn std::error::Error>> {
        Ok(self.client.get_balance(pubkey)?)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Initializing test environment...");
    let context = TestContext::new();

    println!("Setting up accounts...");
    context.setup().await?;

    // Create subscription ID and PDA
    let subscription_id = "premium_content".to_string();
    let (subscription_pda, _) = context.find_subscription_pda(&subscription_id);

    println!("\nInitial setup");
    println!("Subscription PDA: {}", subscription_pda);
    context
        .get_balances(&subscription_pda, "INITIAL SETUP", true)
        .await?;

    // Run all tests
    test_start_subscription(&context, &subscription_id).await?;
    test_make_first_five_payments(&context, &subscription_id).await?;
    test_make_direct_payments(&context, &subscription_id).await?;
    test_cancel_subscription(&context, &subscription_id).await?;
    test_failed_withdrawal(&context, &subscription_id).await?;
    test_successful_withdrawal(&context).await?;

    Ok(())
}

async fn test_start_subscription(
    context: &TestContext,
    subscription_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\nTesting Start Subscription...");
    let (subscription_pda, _) = context.find_subscription_pda(subscription_id);

    // let pre_balances = context
    //     .get_balances(&subscription_pda, "BEFORE SUBSCRIPTION START", true)
    //     .await?;

    // Create instruction data
    let sighash = get_instruction_sighash("start_subscription");
    let args = StartSubscriptionArgs {
        subscription_id: subscription_id.to_string(),
        validation_threshold: DEFAULT_VALIDATION_THRESHOLD,
    };

    let mut instruction_data = Vec::with_capacity(8 + args.try_to_vec()?.len());
    instruction_data.extend_from_slice(&sighash);
    instruction_data.extend_from_slice(&args.try_to_vec()?);

    let instruction = Instruction {
        program_id: context.program_id,
        accounts: vec![
            AccountMeta::new(subscription_pda, false),
            AccountMeta::new(context.buyer.pubkey(), true),
            AccountMeta::new_readonly(context.seller.pubkey(), false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data: instruction_data,
    };

    let recent_blockhash = context.client.get_latest_blockhash()?;
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&context.buyer.pubkey()),
        &[&context.buyer],
        recent_blockhash,
    );

    let signature = context.client.send_and_confirm_transaction(&transaction)?;
    println!("✅ Subscription started. Signature: {}", signature);

    // let post_balances = context
    //     .get_balances(&subscription_pda, "AFTER SUBSCRIPTION START", true)
    //     .await?;

    // Verify account data
    let account_data = context.client.get_account_data(&subscription_pda)?;
    let escrow_account = EscrowAccount::try_deserialize(&mut &account_data[..])?;

    assert_eq!(escrow_account.seller, context.seller.pubkey());
    assert_eq!(escrow_account.buyer, context.buyer.pubkey());
    assert_eq!(escrow_account.subscription_id, subscription_id);
    assert_eq!(escrow_account.payment_count, 0);
    assert_eq!(escrow_account.is_active, true);

    Ok(())
}

async fn test_make_first_five_payments(
    context: &TestContext,
    subscription_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let (subscription_pda, _) = context.find_subscription_pda(subscription_id);
    let payment_amount = LAMPORTS_PER_SOL; // 1 SOL

    for i in 0..5 {
        println!("\nMaking payment {} of 5...", i + 1);

        // Get balances before payment
        let pre_balances = context
            .get_balances(
                &subscription_pda,
                &format!("BEFORE PAYMENT {}", i + 1),
                false,
            )
            .await?;

        // Create payment instruction
        let sighash = get_instruction_sighash("make_payment");
        let args = MakePaymentArgs {
            amount: payment_amount,
        };

        let mut instruction_data = Vec::with_capacity(8 + args.try_to_vec()?.len());
        instruction_data.extend_from_slice(&sighash);
        instruction_data.extend_from_slice(&args.try_to_vec()?);

        let instruction = Instruction {
            program_id: context.program_id,
            accounts: vec![
                AccountMeta::new(subscription_pda, false),
                AccountMeta::new(context.buyer.pubkey(), true),
                AccountMeta::new(context.seller.pubkey(), false),
                AccountMeta::new_readonly(system_program::id(), false),
            ],
            data: instruction_data,
        };

        let recent_blockhash = context.client.get_latest_blockhash()?;
        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&context.buyer.pubkey()),
            &[&context.buyer],
            recent_blockhash,
        );

        let signature = context.client.send_and_confirm_transaction(&transaction)?;
        let post_balances = context
            .get_balances(&subscription_pda, &format!("AFTER PAYMENT {}", i + 1), true)
            .await?;

        // Verify escrow received payment with tolerance for fees
        // Using safe comparison for u64
        let escrow_difference = if post_balances.escrow > pre_balances.escrow {
            post_balances.escrow - pre_balances.escrow
        } else {
            pre_balances.escrow - post_balances.escrow
        };

        let acceptable_range = 10000; // Tolerance for fees
        if escrow_difference > payment_amount + acceptable_range
            || escrow_difference < payment_amount.saturating_sub(acceptable_range)
        {
            return Err(format!(
                "Payment {} escrow amount mismatch. Expected increase: {}, Actual: {}",
                i + 1,
                payment_amount,
                escrow_difference
            )
            .into());
        }

        // Verify seller balance didn't change for escrow payments
        if post_balances.seller != pre_balances.seller {
            return Err(format!(
                "Payment {} seller balance changed unexpectedly. Pre: {}, Post: {}",
                i + 1,
                pre_balances.seller,
                post_balances.seller
            )
            .into());
        }

        println!(
            "✅ Payment {} successful. Signature: {}\n   Amount: {} SOL\n   Escrow increase: {} SOL\n   Seller balance unchanged: {} SOL",
            i + 1,
            signature,
            payment_amount as f64 / LAMPORTS_PER_SOL as f64,
            escrow_difference as f64 / LAMPORTS_PER_SOL as f64,
            post_balances.seller as f64 / LAMPORTS_PER_SOL as f64
        );

        // Add a small delay between payments
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }

    // Final verification of escrow account data
    let account_data = context.client.get_account_data(&subscription_pda)?;
    let escrow_account = EscrowAccount::try_deserialize(&mut &account_data[..])?;

    // Verify payment count
    assert_eq!(
        escrow_account.payment_count, 5,
        "Expected 5 payments, found {}",
        escrow_account.payment_count
    );

    // Verify total amount in escrow
    let expected_total = payment_amount * 5;
    assert_eq!(
        escrow_account.total_amount,
        expected_total,
        "Expected total amount {} SOL, found {} SOL",
        expected_total as f64 / LAMPORTS_PER_SOL as f64,
        escrow_account.total_amount as f64 / LAMPORTS_PER_SOL as f64
    );

    println!("\n✅ All 5 payments completed and verified successfully!");
    println!(
        "   Total in escrow: {} SOL",
        escrow_account.total_amount as f64 / LAMPORTS_PER_SOL as f64
    );

    Ok(())
}

async fn test_make_direct_payments(
    context: &TestContext,
    subscription_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let (subscription_pda, _) = context.find_subscription_pda(subscription_id);
    let payment_amount = LAMPORTS_PER_SOL; // 1 SOL

    for i in 5..7 {
        println!("\nMaking direct payment {} ...", i + 1);
        let pre_balances = context
            .get_balances(
                &subscription_pda,
                &format!("BEFORE DIRECT PAYMENT {}", i + 1),
                false,
            )
            .await?;

        // Create payment instruction
        let sighash = get_instruction_sighash("make_payment");
        let args = MakePaymentArgs {
            amount: payment_amount,
        };

        let mut instruction_data = Vec::with_capacity(8 + args.try_to_vec()?.len());
        instruction_data.extend_from_slice(&sighash);
        instruction_data.extend_from_slice(&args.try_to_vec()?);

        let instruction = Instruction {
            program_id: context.program_id,
            accounts: vec![
                AccountMeta::new(subscription_pda, false),
                AccountMeta::new(context.buyer.pubkey(), true),
                AccountMeta::new(context.seller.pubkey(), false),
                AccountMeta::new_readonly(system_program::id(), false),
            ],
            data: instruction_data,
        };

        let recent_blockhash = context.client.get_latest_blockhash()?;
        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&context.buyer.pubkey()),
            &[&context.buyer],
            recent_blockhash,
        );

        let signature = context.client.send_and_confirm_transaction(&transaction)?;
        let post_balances = context
            .get_balances(
                &subscription_pda,
                &format!("AFTER DIRECT PAYMENT {}", i + 1),
                true,
            )
            .await?;

        // Verify seller received payment directly
        let seller_difference = if post_balances.seller > pre_balances.seller {
            post_balances.seller - pre_balances.seller
        } else {
            pre_balances.seller - post_balances.seller
        };

        let acceptable_range = 10000; // Tolerance for fees
        if seller_difference > payment_amount + acceptable_range
            || seller_difference < payment_amount.saturating_sub(acceptable_range)
        {
            return Err(format!(
                "Direct payment {} seller amount mismatch. Expected increase: {}, Actual: {}",
                i + 1,
                payment_amount,
                seller_difference
            )
            .into());
        }

        // Verify escrow balance didn't change
        if post_balances.escrow != pre_balances.escrow {
            return Err(format!(
                "Direct payment {} escrow balance changed unexpectedly. Pre: {}, Post: {}",
                i + 1,
                pre_balances.escrow,
                post_balances.escrow
            )
            .into());
        }

        println!(
            "✅ Direct payment {} successful. Signature: {}\n   Amount: {} SOL\n   Seller increase: {} SOL\n   Escrow unchanged: {} SOL",
            i + 1,
            signature,
            payment_amount as f64 / LAMPORTS_PER_SOL as f64,
            seller_difference as f64 / LAMPORTS_PER_SOL as f64,
            post_balances.escrow as f64 / LAMPORTS_PER_SOL as f64
        );

        // Add a small delay between payments
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }

    // Final verification of payment count
    let account_data = context.client.get_account_data(&subscription_pda)?;
    let escrow_account = EscrowAccount::try_deserialize(&mut &account_data[..])?;

    assert_eq!(
        escrow_account.payment_count, 7,
        "Expected 7 total payments, found {}",
        escrow_account.payment_count
    );

    println!("\n✅ Both direct payments completed successfully!");
    Ok(())
}

async fn test_cancel_subscription(
    context: &TestContext,
    subscription_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\nTesting Cancel Subscription...");
    let (subscription_pda, _) = context.find_subscription_pda(subscription_id);

    let pre_balances = context
        .get_balances(&subscription_pda, "BEFORE CANCELLATION", true)
        .await?;

    // Create cancel instruction
    let sighash = get_instruction_sighash("cancel_subscription");
    let mut instruction_data = Vec::with_capacity(8);
    instruction_data.extend_from_slice(&sighash);

    let instruction = Instruction {
        program_id: context.program_id,
        accounts: vec![
            AccountMeta::new(subscription_pda, false),
            AccountMeta::new(context.buyer.pubkey(), true),
            AccountMeta::new(context.seller.pubkey(), false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data: instruction_data,
    };

    let recent_blockhash = context.client.get_latest_blockhash()?;
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&context.buyer.pubkey()),
        &[&context.buyer],
        recent_blockhash,
    );

    let signature = context.client.send_and_confirm_transaction(&transaction)?;
    println!("✅ Cancel transaction confirmed. Signature: {}", signature);

    let post_balances = context
        .get_balances(&subscription_pda, "AFTER CANCELLATION", true)
        .await?;

    // Verify account data
    let account_data = context.client.get_account_data(&subscription_pda)?;
    let escrow_account = EscrowAccount::try_deserialize(&mut &account_data[..])?;

    // Verify subscription is inactive
    assert!(
        !escrow_account.is_active,
        "Subscription should be inactive after cancellation"
    );

    // Verify balances haven't changed
    assert_eq!(
        pre_balances.escrow, post_balances.escrow,
        "Escrow balance should not change on cancellation"
    );
    assert_eq!(
        pre_balances.seller, post_balances.seller,
        "Seller balance should not change on cancellation"
    );

    println!("\n✅ Subscription cancelled successfully!");
    println!(
        "   Escrow balance: {} SOL",
        post_balances.escrow as f64 / LAMPORTS_PER_SOL as f64
    );
    println!("   Is active: false");

    Ok(())
}

async fn test_failed_withdrawal(
    context: &TestContext,
    subscription_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\nTesting Failed Withdrawal (Scammer Scenario)...");
    let (subscription_pda, _) = context.find_subscription_pda(subscription_id);

    let pre_balances = context
        .get_balances(&subscription_pda, "BEFORE FAILED WITHDRAWAL", true)
        .await?;

    // Calculate expected escrow total (1 SOL * 5 payments = 5 SOL)
    let expected_escrow_total = LAMPORTS_PER_SOL * 5;

    // Get the rent amount
    let rent_exemption = context
        .client
        .get_minimum_balance_for_rent_exemption(EscrowAccount::default().try_to_vec()?.len())?;

    println!("\nPre-withdrawal balances:");
    println!(
        "Seller: {} SOL",
        pre_balances.seller as f64 / LAMPORTS_PER_SOL as f64
    );
    println!(
        "Escrow: {} SOL",
        pre_balances.escrow as f64 / LAMPORTS_PER_SOL as f64
    );
    println!(
        "Buyer: {} SOL",
        pre_balances.buyer as f64 / LAMPORTS_PER_SOL as f64
    );
    println!(
        "Rent amount: {} SOL",
        rent_exemption as f64 / LAMPORTS_PER_SOL as f64
    );

    // Create withdraw instruction with validation data above threshold
    let sighash = get_instruction_sighash("withdraw_funds");
    let args = WithdrawFundsArgs {
        validation_data: 2000, // Higher than threshold of 1000
    };

    let mut instruction_data = Vec::with_capacity(8 + args.try_to_vec()?.len());
    instruction_data.extend_from_slice(&sighash);
    instruction_data.extend_from_slice(&args.try_to_vec()?);

    let instruction = Instruction {
        program_id: context.program_id,
        accounts: vec![
            AccountMeta::new(subscription_pda, false),
            AccountMeta::new(context.buyer.pubkey(), false),
            AccountMeta::new(context.seller.pubkey(), true),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data: instruction_data,
    };

    let recent_blockhash = context.client.get_latest_blockhash()?;
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&context.seller.pubkey()),
        &[&context.seller],
        recent_blockhash,
    );

    let signature = context.client.send_and_confirm_transaction(&transaction)?;
    println!(
        "✅ Withdrawal transaction confirmed. Signature: {}",
        signature
    );

    let post_balances = context
        .get_balances(&subscription_pda, "AFTER FAILED WITHDRAWAL", true)
        .await?;

    // Verify funds were returned to buyer
    let expected_buyer_increase = expected_escrow_total + rent_exemption;
    let buyer_difference = if post_balances.buyer > pre_balances.buyer {
        post_balances.buyer - pre_balances.buyer
    } else {
        pre_balances.buyer - post_balances.buyer
    };

    let acceptable_range = LAMPORTS_PER_SOL / 100; // Tolerance for fees (0.01 SOL)
    if buyer_difference > expected_buyer_increase + acceptable_range
        || buyer_difference < expected_buyer_increase.saturating_sub(acceptable_range)
    {
        println!("❌ Buyer balance mismatch:");
        println!(
            "   Expected increase: {} SOL",
            expected_buyer_increase as f64 / LAMPORTS_PER_SOL as f64
        );
        println!(
            "   Actual increase: {} SOL",
            buyer_difference as f64 / LAMPORTS_PER_SOL as f64
        );
        println!(
            "   Difference: {} SOL",
            (expected_buyer_increase as i128 - buyer_difference as i128).abs() as f64
                / LAMPORTS_PER_SOL as f64
        );
        return Err("Buyer balance mismatch".into());
    }

    // Verify seller only paid transaction fees but didn't receive funds
    let seller_difference = if pre_balances.seller > post_balances.seller {
        pre_balances.seller - post_balances.seller
    } else {
        post_balances.seller - pre_balances.seller
    };

    // Allow for transaction fee (typically less than 0.01 SOL)
    let max_expected_fee = LAMPORTS_PER_SOL / 100; // 0.01 SOL
    assert!(
        seller_difference <= max_expected_fee,
        "Seller balance changed by {} SOL, which is more than expected transaction fee of {} SOL",
        seller_difference as f64 / LAMPORTS_PER_SOL as f64,
        max_expected_fee as f64 / LAMPORTS_PER_SOL as f64
    );

    // Verify escrow account is closed
    assert_eq!(post_balances.escrow, 0, "Escrow account should be closed");

    println!("\n✅ Failed withdrawal test completed successfully!");
    println!(
        "   Funds returned to buyer: {} SOL",
        buyer_difference as f64 / LAMPORTS_PER_SOL as f64
    );

    Ok(())
}

async fn test_successful_withdrawal(
    context: &TestContext,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\nTesting Successful Withdrawal...");

    // Generate new keypairs for buyer and seller
    let new_buyer = Keypair::new();
    let new_seller = Keypair::new();

    // Fund new accounts using the same method as setup
    println!("\nFunding new buyer account...");
    context
        .request_airdrop_with_confirmation(&new_buyer.pubkey(), BUYER_INITIAL_BALANCE)
        .await?;

    println!("\nFunding new seller account...");
    context
        .request_airdrop_with_confirmation(&new_seller.pubkey(), SELLER_INITIAL_BALANCE)
        .await?;

    // Verify initial balances
    let buyer_balance = context.client.get_balance(&new_buyer.pubkey())?;
    let seller_balance = context.client.get_balance(&new_seller.pubkey())?;

    println!("\nInitial balances:");
    println!(
        "New Buyer: {} SOL",
        buyer_balance as f64 / LAMPORTS_PER_SOL as f64
    );
    println!(
        "New Seller: {} SOL",
        seller_balance as f64 / LAMPORTS_PER_SOL as f64
    );

    // Generate new subscription for this test
    let subscription_id = "premium_content_2".to_string();
    let subscription_pda = Pubkey::find_program_address(
        &[
            b"escrow",
            new_buyer.pubkey().as_ref(),
            new_seller.pubkey().as_ref(),
            subscription_id.as_bytes(),
        ],
        &context.program_id,
    )
    .0;

    // Start subscription
    println!("\nStarting new subscription...");
    let sighash = get_instruction_sighash("start_subscription");
    let start_args = StartSubscriptionArgs {
        subscription_id: subscription_id.clone(),
        validation_threshold: DEFAULT_VALIDATION_THRESHOLD,
    };

    let mut instruction_data = Vec::with_capacity(8 + start_args.try_to_vec()?.len());
    instruction_data.extend_from_slice(&sighash);
    instruction_data.extend_from_slice(&start_args.try_to_vec()?);

    let instruction = Instruction {
        program_id: context.program_id,
        accounts: vec![
            AccountMeta::new(subscription_pda, false),
            AccountMeta::new(new_buyer.pubkey(), true),
            AccountMeta::new_readonly(new_seller.pubkey(), false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data: instruction_data,
    };

    let recent_blockhash = context.client.get_latest_blockhash()?;
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&new_buyer.pubkey()),
        &[&new_buyer],
        recent_blockhash,
    );

    let signature = context.client.send_and_confirm_transaction(&transaction)?;
    println!("✅ Subscription started. Signature: {}", signature);

    // Make 5 payments
    let payment_amount = LAMPORTS_PER_SOL; // 1 SOL per payment
    for i in 0..5 {
        println!("\nMaking payment {} of 5...", i + 1);
        let sighash = get_instruction_sighash("make_payment");
        let args = MakePaymentArgs {
            amount: payment_amount,
        };

        let mut instruction_data = Vec::with_capacity(8 + args.try_to_vec()?.len());
        instruction_data.extend_from_slice(&sighash);
        instruction_data.extend_from_slice(&args.try_to_vec()?);

        let instruction = Instruction {
            program_id: context.program_id,
            accounts: vec![
                AccountMeta::new(subscription_pda, false),
                AccountMeta::new(new_buyer.pubkey(), true),
                AccountMeta::new(new_seller.pubkey(), false),
                AccountMeta::new_readonly(system_program::id(), false),
            ],
            data: instruction_data,
        };

        let recent_blockhash = context.client.get_latest_blockhash()?;
        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&new_buyer.pubkey()),
            &[&new_buyer],
            recent_blockhash,
        );

        let signature = context.client.send_and_confirm_transaction(&transaction)?;
        println!("✅ Payment {} completed. Signature: {}", i + 1, signature);
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }

    // Cancel subscription
    println!("\nCancelling subscription...");
    let sighash = get_instruction_sighash("cancel_subscription");
    let mut instruction_data = Vec::with_capacity(8);
    instruction_data.extend_from_slice(&sighash);

    let instruction = Instruction {
        program_id: context.program_id,
        accounts: vec![
            AccountMeta::new(subscription_pda, false),
            AccountMeta::new(new_buyer.pubkey(), true),
            AccountMeta::new(new_seller.pubkey(), false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data: instruction_data,
    };

    let recent_blockhash = context.client.get_latest_blockhash()?;
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&new_buyer.pubkey()),
        &[&new_buyer],
        recent_blockhash,
    );

    let signature = context.client.send_and_confirm_transaction(&transaction)?;
    println!("✅ Subscription cancelled. Signature: {}", signature);

    // Get pre-withdrawal balances
    let pre_balances = Balance {
        seller: context.client.get_balance(&new_seller.pubkey())?,
        escrow: context.client.get_balance(&subscription_pda)?,
        buyer: context.client.get_balance(&new_buyer.pubkey())?,
    };

    println!("\nPre-withdrawal balances:");
    println!(
        "Seller: {} SOL",
        pre_balances.seller as f64 / LAMPORTS_PER_SOL as f64
    );
    println!(
        "Escrow: {} SOL",
        pre_balances.escrow as f64 / LAMPORTS_PER_SOL as f64
    );
    println!(
        "Buyer: {} SOL",
        pre_balances.buyer as f64 / LAMPORTS_PER_SOL as f64
    );

    // Get the rent amount
    let rent_exemption = context
        .client
        .get_minimum_balance_for_rent_exemption(EscrowAccount::default().try_to_vec()?.len())?;

    // Execute successful withdrawal
    println!("\nExecuting withdrawal with valid validation data...");
    let sighash = get_instruction_sighash("withdraw_funds");
    let args = WithdrawFundsArgs {
        validation_data: 500, // Lower than threshold of 1000
    };

    let mut instruction_data = Vec::with_capacity(8 + args.try_to_vec()?.len());
    instruction_data.extend_from_slice(&sighash);
    instruction_data.extend_from_slice(&args.try_to_vec()?);

    let instruction = Instruction {
        program_id: context.program_id,
        accounts: vec![
            AccountMeta::new(subscription_pda, false),
            AccountMeta::new(new_buyer.pubkey(), false),
            AccountMeta::new(new_seller.pubkey(), true),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data: instruction_data,
    };

    let recent_blockhash = context.client.get_latest_blockhash()?;
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&new_seller.pubkey()),
        &[&new_seller],
        recent_blockhash,
    );

    let signature = context.client.send_and_confirm_transaction(&transaction)?;
    println!(
        "✅ Withdrawal transaction confirmed. Signature: {}",
        signature
    );

    let post_balances = Balance {
        seller: context.client.get_balance(&new_seller.pubkey())?,
        escrow: context.client.get_balance(&subscription_pda)?,
        buyer: context.client.get_balance(&new_buyer.pubkey())?,
    };

    println!("\nPost-withdrawal balances:");
    println!(
        "Seller: {} SOL",
        post_balances.seller as f64 / LAMPORTS_PER_SOL as f64
    );
    println!(
        "Escrow: {} SOL",
        post_balances.escrow as f64 / LAMPORTS_PER_SOL as f64
    );
    println!(
        "Buyer: {} SOL",
        post_balances.buyer as f64 / LAMPORTS_PER_SOL as f64
    );

    // Verify seller received escrow funds
    let expected_seller_increase = LAMPORTS_PER_SOL * 5; // 5 SOL total
    let seller_difference = if post_balances.seller > pre_balances.seller {
        post_balances.seller - pre_balances.seller
    } else {
        pre_balances.seller - post_balances.seller
    };

    let acceptable_range = LAMPORTS_PER_SOL / 100; // 0.01 SOL tolerance
    if seller_difference > expected_seller_increase + acceptable_range
        || seller_difference < expected_seller_increase.saturating_sub(acceptable_range)
    {
        println!("❌ Seller balance mismatch:");
        println!(
            "   Expected increase: {} SOL",
            expected_seller_increase as f64 / LAMPORTS_PER_SOL as f64
        );
        println!(
            "   Actual increase: {} SOL",
            seller_difference as f64 / LAMPORTS_PER_SOL as f64
        );
        return Err("Seller balance mismatch".into());
    }

    // Verify buyer received rent
    let buyer_difference = if post_balances.buyer > pre_balances.buyer {
        post_balances.buyer - pre_balances.buyer
    } else {
        pre_balances.buyer - post_balances.buyer
    };

    assert!(
        buyer_difference >= rent_exemption.saturating_sub(acceptable_range),
        "Buyer should receive rent amount"
    );

    // Verify escrow account is closed
    assert_eq!(post_balances.escrow, 0, "Escrow account should be closed");

    println!("\n✅ Successful withdrawal test completed!");
    println!(
        "   Seller received: {} SOL",
        seller_difference as f64 / LAMPORTS_PER_SOL as f64
    );
    println!(
        "   Buyer received rent: {} SOL",
        buyer_difference as f64 / LAMPORTS_PER_SOL as f64
    );

    Ok(())
}
