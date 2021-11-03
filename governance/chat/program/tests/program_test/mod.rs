use std::str::FromStr;

use gemachain_program::{program_error::ProgramError, pubkey::Pubkey};
use gemachain_program_test::processor;

use gemachain_sdk::{signature::Keypair, signer::Signer};
use spl_governance::{
    instruction::{
        create_account_governance, create_proposal, create_realm, deposit_governing_tokens,
    },
    state::{
        enums::{MintMaxVoteWeightSource, VoteThresholdPercentage},
        governance::{get_account_governance_address, GovernanceConfig},
        proposal::get_proposal_address,
        realm::get_realm_address,
        token_owner_record::get_token_owner_record_address,
    },
};
use spl_governance_chat::{
    instruction::post_message,
    processor::process_instruction,
    state::{ChatMessage, GovernanceChatAccountType, MessageBody},
};
use spl_governance_test_sdk::{ProgramTestBench, TestBenchProgram};

use crate::program_test::cookies::{ChatMessageCookie, ProposalCookie};

use self::cookies::TokenOwnerRecordCookie;

pub mod cookies;

pub struct GovernanceChatProgramTest {
    pub bench: ProgramTestBench,
    pub program_id: Pubkey,
    pub governance_program_id: Pubkey,
}

impl GovernanceChatProgramTest {
    pub async fn start_new() -> Self {
        let program_id = Pubkey::from_str("GovernanceChat11111111111111111111111111111").unwrap();

        let chat_program = TestBenchProgram {
            program_name: "spl_governance_chat",
            program_id: program_id,
            process_instruction: processor!(process_instruction),
        };

        let governance_program_id =
            Pubkey::from_str("Governance111111111111111111111111111111111").unwrap();
        let governance_program = TestBenchProgram {
            program_name: "spl_governance",
            program_id: governance_program_id,
            process_instruction: processor!(spl_governance::processor::process_instruction),
        };

        let bench = ProgramTestBench::start_new(&[chat_program, governance_program]).await;

        Self {
            bench,
            program_id,
            governance_program_id,
        }
    }

    #[allow(dead_code)]
    pub async fn with_proposal(&mut self) -> ProposalCookie {
        // Create Realm
        let name = self.bench.get_unique_name("realm");

        let realm_address = get_realm_address(&self.governance_program_id, &name);

        let governing_token_mint_keypair = Keypair::new();
        let governing_token_mint_authority = Keypair::new();

        self.bench
            .create_mint(
                &governing_token_mint_keypair,
                &governing_token_mint_authority.pubkey(),
            )
            .await;

        let realm_authority = Keypair::new();

        let create_realm_ix = create_realm(
            &self.governance_program_id,
            &realm_authority.pubkey(),
            &governing_token_mint_keypair.pubkey(),
            &self.bench.payer.pubkey(),
            None,
            None,
            name.clone(),
            1,
            MintMaxVoteWeightSource::FULL_SUPPLY_FRACTION,
        );

        self.bench
            .process_transaction(&[create_realm_ix], None)
            .await
            .unwrap();

        // Create TokenOwnerRecord
        let token_owner = Keypair::new();
        let token_source = Keypair::new();

        let transfer_authority = Keypair::new();
        let amount = 100;

        self.bench
            .create_token_account_with_transfer_authority(
                &token_source,
                &governing_token_mint_keypair.pubkey(),
                &governing_token_mint_authority,
                amount,
                &token_owner,
                &transfer_authority.pubkey(),
            )
            .await;

        let deposit_governing_tokens_ix = deposit_governing_tokens(
            &self.governance_program_id,
            &realm_address,
            &token_source.pubkey(),
            &token_owner.pubkey(),
            &token_owner.pubkey(),
            &self.bench.payer.pubkey(),
            amount,
            &governing_token_mint_keypair.pubkey(),
        );

        self.bench
            .process_transaction(&[deposit_governing_tokens_ix], Some(&[&token_owner]))
            .await
            .unwrap();

        // Create Governance
        let governed_account_address = Pubkey::new_unique();

        let governance_config = GovernanceConfig {
            min_community_tokens_to_create_proposal: 5,
            min_council_tokens_to_create_proposal: 2,
            min_instruction_hold_up_time: 10,
            max_voting_time: 10,
            vote_threshold_percentage: VoteThresholdPercentage::YesVote(60),
            vote_weight_source: spl_governance::state::enums::VoteWeightSource::Deposit,
            proposal_cool_off_time: 0,
        };

        let token_owner_record_address = get_token_owner_record_address(
            &self.governance_program_id,
            &realm_address,
            &governing_token_mint_keypair.pubkey(),
            &token_owner.pubkey(),
        );

        let create_account_governance_ix = create_account_governance(
            &self.governance_program_id,
            &realm_address,
            &governed_account_address,
            &token_owner_record_address,
            &self.bench.payer.pubkey(),
            &token_owner.pubkey(),
            None,
            governance_config,
        );

        self.bench
            .process_transaction(&[create_account_governance_ix], Some(&[&token_owner]))
            .await
            .unwrap();

        // Create Proposal

        let governance_address = get_account_governance_address(
            &self.governance_program_id,
            &realm_address,
            &governed_account_address,
        );

        let proposal_name = "Proposal #1".to_string();
        let description_link = "Proposal Description".to_string();
        let proposal_index: u32 = 0;

        let create_proposal_ix = create_proposal(
            &self.governance_program_id,
            &governance_address,
            &token_owner_record_address,
            &token_owner.pubkey(),
            &self.bench.payer.pubkey(),
            None,
            &realm_address,
            proposal_name,
            description_link.clone(),
            &governing_token_mint_keypair.pubkey(),
            proposal_index,
        );

        self.bench
            .process_transaction(&[create_proposal_ix], Some(&[&token_owner]))
            .await
            .unwrap();

        let proposal_address = get_proposal_address(
            &self.governance_program_id,
            &governance_address,
            &governing_token_mint_keypair.pubkey(),
            &proposal_index.to_le_bytes(),
        );

        ProposalCookie {
            address: proposal_address,
            realm_address,
            governance_address,
            token_owner_record_address,
            token_owner,
            governing_token_mint: governing_token_mint_keypair.pubkey(),
            governing_token_mint_authority: governing_token_mint_authority,
        }
    }

    #[allow(dead_code)]
    pub async fn with_token_owner_deposit(
        &mut self,
        proposal_cookie: &ProposalCookie,
        deposit_amount: u64,
    ) -> TokenOwnerRecordCookie {
        let token_owner = Keypair::new();
        let token_source = Keypair::new();

        let transfer_authority = Keypair::new();

        self.bench
            .create_token_account_with_transfer_authority(
                &token_source,
                &proposal_cookie.governing_token_mint,
                &proposal_cookie.governing_token_mint_authority,
                deposit_amount,
                &token_owner,
                &transfer_authority.pubkey(),
            )
            .await;

        let deposit_governing_tokens_ix = deposit_governing_tokens(
            &self.governance_program_id,
            &proposal_cookie.realm_address,
            &token_source.pubkey(),
            &token_owner.pubkey(),
            &token_owner.pubkey(),
            &self.bench.payer.pubkey(),
            deposit_amount,
            &proposal_cookie.governing_token_mint,
        );

        self.bench
            .process_transaction(&[deposit_governing_tokens_ix], Some(&[&token_owner]))
            .await
            .unwrap();

        let token_owner_record_address = get_token_owner_record_address(
            &self.governance_program_id,
            &proposal_cookie.realm_address,
            &proposal_cookie.governing_token_mint,
            &token_owner.pubkey(),
        );
        TokenOwnerRecordCookie {
            address: token_owner_record_address,
            token_owner,
        }
    }

    #[allow(dead_code)]
    pub async fn with_chat_message(
        &mut self,
        proposal_cookie: &ProposalCookie,
        reply_to: Option<Pubkey>,
    ) -> Result<ChatMessageCookie, ProgramError> {
        let message_account = Keypair::new();
        let message_body = MessageBody::Text("My comment".to_string());

        let post_message_ix = post_message(
            &self.program_id,
            &self.governance_program_id,
            &proposal_cookie.governance_address,
            &proposal_cookie.address,
            &proposal_cookie.token_owner_record_address,
            &proposal_cookie.token_owner.pubkey(),
            reply_to,
            &message_account.pubkey(),
            &self.bench.payer.pubkey(),
            message_body.clone(),
        );

        let clock = self.bench.get_clock().await;

        let message = ChatMessage {
            account_type: GovernanceChatAccountType::ChatMessage,
            proposal: proposal_cookie.address,
            author: proposal_cookie.token_owner.pubkey(),
            posted_at: clock.unix_timestamp,
            reply_to,
            body: message_body,
        };

        self.bench
            .process_transaction(
                &[post_message_ix],
                Some(&[&proposal_cookie.token_owner, &message_account]),
            )
            .await?;

        Ok(ChatMessageCookie {
            address: message_account.pubkey(),
            account: message,
        })
    }

    #[allow(dead_code)]
    pub async fn get_message_account(&mut self, message_address: &Pubkey) -> ChatMessage {
        self.bench
            .get_borsh_account::<ChatMessage>(message_address)
            .await
    }
}
