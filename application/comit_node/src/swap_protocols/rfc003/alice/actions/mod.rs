use crate::swap_protocols::rfc003::actions::Action;

mod erc20_actions;
mod non_erc20_actions;

#[derive(Debug, Clone, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub enum ActionKind<Deploy, Fund, Redeem, Refund> {
    Deploy(Deploy),
    Fund(Fund),
    Redeem(Redeem),
    Refund(Refund),
}

impl<Deploy, Fund, Redeem, Refund> ActionKind<Deploy, Fund, Redeem, Refund> {
    fn into_action(self) -> Action<ActionKind<Deploy, Fund, Redeem, Refund>> {
        Action {
            inner: self,
            invalid_until: None,
        }
    }
}
