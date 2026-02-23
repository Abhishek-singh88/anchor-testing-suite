use anchor_lang::prelude::*;
use anchor_lang::system_program;

declare_id!("Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS");

#[program]
pub mod test_vault {
    use super::*;

    pub fn initialize_vault(ctx: Context<InitializeVault>) -> Result<()> {
        let vault = &mut ctx.accounts.vault;
        vault.authority = ctx.accounts.user.key();
        vault.balance = 0;
        vault.last_update = Clock::get()?.unix_timestamp;
        msg!("Vault initialized for {}", vault.authority);
        Ok(())
    }

    pub fn deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
        let user_info = ctx.accounts.user.to_account_info();
        let vault_info = ctx.accounts.vault.to_account_info();
        let cpi_accounts = system_program::Transfer {
            from: user_info,
            to: vault_info,
        };
        let cpi_program = ctx.accounts.system_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        system_program::transfer(cpi_ctx, amount)?;

        let vault = &mut ctx.accounts.vault;
        vault.balance += amount;
        vault.last_update = Clock::get()?.unix_timestamp;
        msg!("Deposited {} lamports. New balance: {}", amount, vault.balance);
        Ok(())
    }

    pub fn withdraw(ctx: Context<Withdraw>, amount: u64) -> Result<()> {
        require!(ctx.accounts.vault.balance >= amount, VaultError::InsufficientFunds);
        require!(ctx.accounts.vault.authority == ctx.accounts.user.key(), VaultError::Unauthorized);
        
        let user_key = ctx.accounts.user.key();
        let seeds = &[
            b"vault".as_ref(),
            user_key.as_ref(),
            &[ctx.bumps.vault],
        ];
        let signer = &[&seeds[..]];
        
        let cpi_accounts = anchor_lang::system_program::Transfer {
            from: ctx.accounts.vault.to_account_info(),
            to: ctx.accounts.user.to_account_info(),
        };
        let cpi_program = ctx.accounts.system_program.to_account_info();
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
        anchor_lang::system_program::transfer(cpi_ctx, amount)?;
        
        ctx.accounts.vault.balance -= amount;
        ctx.accounts.vault.last_update = Clock::get()?.unix_timestamp;
        msg!("Withdrew {}. New balance: {}", amount, ctx.accounts.vault.balance);
        Ok(())
    }
}

#[derive(Accounts)]
pub struct InitializeVault<'info> {
    #[account(
        init,
        payer = user,
        space = 8 + 32 + 8 + 8,
        seeds = [b"vault", user.key().as_ref()],
        bump
    )]
    pub vault: Account<'info, Vault>,
    #[account(mut)]
    pub user: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(
        mut,
        seeds = [b"vault", user.key().as_ref()],
        bump
    )]
    pub vault: Account<'info, Vault>,
    #[account(mut)]
    pub user: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Withdraw<'info> {
    #[account(
        mut,
        seeds = [b"vault", user.key().as_ref()],
        bump
    )]
    pub vault: Account<'info, Vault>,
    #[account(mut)]
    pub user: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[account]
pub struct Vault {
    pub authority: Pubkey,
    pub balance: u64,
    pub last_update: i64,
}

#[error_code]
pub enum VaultError {
    #[msg("Insufficient funds in vault")]
    InsufficientFunds,
    #[msg("Unauthorized withdrawal")]
    Unauthorized,
}
