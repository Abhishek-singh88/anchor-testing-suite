use anchor_lang::prelude::*;

declare_id!("GLnH5huAyPLjEY9rNkWceL5mi9zYwwua5apJcZm1hC51");

#[program]
pub mod anchor_testing_suite {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        msg!("Greetings from: {:?}", ctx.program_id);
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize {}
