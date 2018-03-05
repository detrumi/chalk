use crate::cast::Caster;
use crate::fallible::Fallible;
use crate::ir;
use crate::ir::could_match::CouldMatch;
use crate::solve::infer::instantiate::BindersAndValue;
use crate::solve::infer::ucanonicalize::UCanonicalized;
use crate::solve::slg::{CanonicalGoal, Literal, ExClause, UCanonicalGoal};
use crate::solve::truncate::{self, Truncated};
use crate::fold::Fold;
use std::fmt::Debug;
use std::sync::Arc;

// Notes:
//
// How do we use `Substitution`:
// - we use them as key in a hashmap
// - we ask if they are trivial:
//   - for a given goal
//   - with no region constraints
// - we apply them to a given canonical goal to yield an instantiated goal
//
// How do we use a `UniverseMap`:
// - we apply it to the result we get back from an answer
// - which is then fed into `apply_answer_subst`
//
// How do we use region constraints:
// - opaquely, I imagine
//
// It seems clear we can extract a

crate trait Context: Sized + Clone {
    type InferenceTable: InferenceTable<Self>;
    type InferenceVariable: InferenceVariable<Self>;

    /// True if this is a coinductive goal -- e.g., proving an auto trait.
    fn is_coinductive(&self, goal: &UCanonicalGoal<ir::DomainGoal>) -> bool;

    /// Returns the set of program clauses that might apply to
    /// `goal`. (This set can be over-approximated, naturally.)
    fn program_clauses(
        &self,
        environment: &ir::Environment<ir::DomainGoal>,
        goal: &ir::DomainGoal,
    ) -> Vec<ir::ProgramClause<ir::DomainGoal>>;

    /// If `subgoal` is too large, return a truncated variant (else
    /// return `None`).
    fn truncate_goal(
        &self,
        infer: &mut Self::InferenceTable,
        max_size: usize,
        subgoal: &ir::InEnvironment<ir::Goal<ir::DomainGoal>>,
    ) -> Option<ir::InEnvironment<ir::Goal<ir::DomainGoal>>>;

    /// If `subst` is too large, return a truncated variant (else
    /// return `None`).
    fn truncate_answer(
        &self,
        infer: &mut Self::InferenceTable,
        max_size: usize,
        subst: &ir::Substitution,
    ) -> Option<ir::Substitution>;
}

crate trait InferenceVariable<C: Context>: Copy {
    fn to_ty(self) -> ir::Ty;
    fn to_lifetime(self) -> ir::Lifetime;
}

crate trait InferenceTable<C: Context>: Clone {
    type UnificationResult: UnificationResult<C>;

    fn new() -> Self;

    // Used by: simplify
    fn instantiate_binders_universally<T>(
        &mut self,
        arg: &impl BindersAndValue<Output = T>,
    ) -> T::Result
    where
        T: Fold;

    // Used by: logic
    fn instantiate_universes<'v, T>(
        &mut self,
        value: &'v ir::UCanonical<T>,
    ) -> &'v ir::Canonical<T>;

    // Used by: truncate
    fn max_universe(&self) -> ir::UniverseIndex;

    // Used by: aggregate, truncate
    fn new_variable(&mut self, ui: ir::UniverseIndex) -> C::InferenceVariable;

    // Used by: resolvent
    fn normalize_lifetime(&mut self, leaf: &ir::Lifetime, binders: usize) -> Option<ir::Lifetime>;

    // Used by: resolvent, truncate
    fn normalize_shallow(&mut self, leaf: &ir::Ty, binders: usize) -> Option<ir::Ty>;

    // Used by: resolvent, logic (but for debugging only)
    fn normalize_deep<T: Fold>(&mut self, value: &T) -> T::Result;

    // Used by: logic
    fn canonicalize_goal(
        &mut self,
        value: &ir::InEnvironment<ir::Goal<ir::DomainGoal>>,
    ) -> ir::Canonical<ir::InEnvironment<ir::Goal<ir::DomainGoal>>>;

    // Used by: logic
    fn canonicalize_constrained_subst(
        &mut self,
        value: &ir::ConstrainedSubst,
    ) -> ir::Canonical<ir::ConstrainedSubst>;

    // Used by: logic
    fn u_canonicalize_goal(
        &mut self,
        value: &CanonicalGoal<ir::DomainGoal>,
    ) -> UCanonicalized<ir::InEnvironment<ir::Goal<ir::DomainGoal>>>;

    // Used by: logic
    fn fresh_subst(&mut self, binders: &[ir::ParameterKind<ir::UniverseIndex>])
        -> ir::Substitution;

    // Used by: logic
    fn invert_goal(
        &mut self,
        value: &ir::InEnvironment<ir::Goal<ir::DomainGoal>>,
    ) -> Option<ir::InEnvironment<ir::Goal<ir::DomainGoal>>>;

    // Used by: simplify, resolvent
    fn instantiate_binders_existentially<T>(
        &mut self,
        arg: &impl BindersAndValue<Output = T>,
    ) -> T::Result
    where
        T: Fold;

    // Used by: resolvent
    fn instantiate_canonical<T>(&mut self, bound: &ir::Canonical<T>) -> T::Result
    where
        T: Fold + Debug;

    fn unify_domain_goals(
        &mut self,
        environment: &Arc<ir::Environment<ir::DomainGoal>>,
        a: &ir::DomainGoal,
        b: &ir::DomainGoal,
    ) -> Fallible<Self::UnificationResult>;

    fn unify_parameters(
        &mut self,
        environment: &Arc<ir::Environment<ir::DomainGoal>>,
        a: &ir::Parameter,
        b: &ir::Parameter,
    ) -> Fallible<Self::UnificationResult>;
}

crate trait UnificationResult<C: Context> {
    fn into_ex_clause(self, ex_clause: &mut ExClause);
}

///////////////////////////////////////////////////////////////////////////

#[allow(dead_code)] // for some reason rustc reports this as dead??
pub type SlgContext = Arc<ir::ProgramEnvironment<ir::DomainGoal>>;

impl Context for Arc<ir::ProgramEnvironment<ir::DomainGoal>> {
    type InferenceTable = ::crate::solve::infer::InferenceTable;
    type InferenceVariable = ::crate::solve::infer::var::InferenceVariable;

    fn is_coinductive(&self, goal: &UCanonicalGoal<ir::DomainGoal>) -> bool {
        goal.is_coinductive(self)
    }

    fn program_clauses(
        &self,
        environment: &ir::Environment<ir::DomainGoal>,
        goal: &ir::DomainGoal,
    ) -> Vec<ir::ProgramClause<ir::DomainGoal>> {
        let environment_clauses = environment
            .clauses
            .iter()
            .filter(|&env_clause| env_clause.could_match(goal))
            .map(|env_clause| env_clause.clone().into_program_clause());

        let program_clauses = self.program_clauses
            .iter()
            .filter(|clause| clause.could_match(goal))
            .cloned();

        environment_clauses.chain(program_clauses).collect()
    }

    /// If `subgoal` is too large, return a truncated variant (else
    /// return `None`).
    fn truncate_goal(
        &self,
        infer: &mut Self::InferenceTable,
        max_size: usize,
        subgoal: &ir::InEnvironment<ir::Goal<ir::DomainGoal>>,
    ) -> Option<ir::InEnvironment<ir::Goal<ir::DomainGoal>>> {
        let Truncated { overflow, value } = truncate::truncate(infer, max_size, subgoal);
        if overflow { Some(value) } else { None }
    }

    /// If `subst` is too large, return a truncated variant (else
    /// return `None`).
    fn truncate_answer(
        &self,
        infer: &mut Self::InferenceTable,
        max_size: usize,
        subst: &ir::Substitution,
    ) -> Option<ir::Substitution> {
        let Truncated { overflow, value } = truncate::truncate(infer, max_size, subst);
        if overflow { Some(value) } else { None }
    }
}

impl InferenceTable<SlgContext> for ::crate::solve::infer::InferenceTable {
    type UnificationResult = ::crate::solve::infer::unify::UnificationResult;

    fn new() -> Self {
        Self::new()
    }

    fn fresh_subst(
        &mut self,
        binders: &[ir::ParameterKind<ir::UniverseIndex>],
    ) -> ir::Substitution {
        self.fresh_subst(binders)
    }

    fn instantiate_binders_universally<T>(
        &mut self,
        arg: &impl BindersAndValue<Output = T>,
    ) -> T::Result
    where
        T: Fold,
    {
        self.instantiate_binders_universally(arg)
    }

    fn instantiate_universes<'v, T>(
        &mut self,
        value: &'v ir::UCanonical<T>,
    ) -> &'v ir::Canonical<T> {
        self.instantiate_universes(value)
    }

    fn max_universe(&self) -> ir::UniverseIndex {
        self.max_universe()
    }

    fn new_variable(
        &mut self,
        ui: ir::UniverseIndex,
    ) -> ::crate::solve::infer::var::InferenceVariable {
        self.new_variable(ui)
    }

    fn normalize_lifetime(&mut self, leaf: &ir::Lifetime, binders: usize) -> Option<ir::Lifetime> {
        self.normalize_lifetime(leaf, binders)
    }

    fn normalize_shallow(&mut self, leaf: &ir::Ty, binders: usize) -> Option<ir::Ty> {
        self.normalize_shallow(leaf, binders)
    }

    fn normalize_deep<T: Fold>(&mut self, value: &T) -> T::Result {
        self.normalize_deep(value)
    }

    fn canonicalize_goal(
        &mut self,
        value: &ir::InEnvironment<ir::Goal<ir::DomainGoal>>,
    ) -> ir::Canonical<ir::InEnvironment<ir::Goal<ir::DomainGoal>>> {
        self.canonicalize(value).quantified
    }

    fn canonicalize_constrained_subst(
        &mut self,
        value: &ir::ConstrainedSubst,
    ) -> ir::Canonical<ir::ConstrainedSubst> {
        self.canonicalize(value).quantified
    }

    fn u_canonicalize_goal(
        &mut self,
        value: &CanonicalGoal<ir::DomainGoal>,
    ) -> UCanonicalized<ir::InEnvironment<ir::Goal<ir::DomainGoal>>> {
        self.u_canonicalize(value)
    }

    fn invert_goal(
        &mut self,
        value: &ir::InEnvironment<ir::Goal<ir::DomainGoal>>,
    ) -> Option<ir::InEnvironment<ir::Goal<ir::DomainGoal>>> {
        self.invert(value)
    }

    fn instantiate_binders_existentially<T>(
        &mut self,
        arg: &impl BindersAndValue<Output = T>,
    ) -> T::Result
    where
        T: Fold,
    {
        self.instantiate_binders_existentially(arg)
    }

    fn instantiate_canonical<T>(&mut self, bound: &ir::Canonical<T>) -> T::Result
    where
        T: Fold + Debug,
    {
        self.instantiate_canonical(bound)
    }

    fn unify_domain_goals(
        &mut self,
        environment: &Arc<ir::Environment<ir::DomainGoal>>,
        a: &ir::DomainGoal,
        b: &ir::DomainGoal,
    ) -> Fallible<Self::UnificationResult> {
        self.unify(environment, a, b)
    }

    fn unify_parameters(
        &mut self,
        environment: &Arc<ir::Environment<ir::DomainGoal>>,
        a: &ir::Parameter,
        b: &ir::Parameter,
    ) -> Fallible<Self::UnificationResult> {
        self.unify(environment, a, b)
    }
}

impl UnificationResult<SlgContext> for ::crate::solve::infer::unify::UnificationResult {
    fn into_ex_clause(self, ex_clause: &mut ExClause) {
        ex_clause
            .subgoals
            .extend(self.goals.into_iter().casted().map(Literal::Positive));
        ex_clause.constraints.extend(self.constraints);
    }
}

impl InferenceVariable<SlgContext> for ::crate::solve::infer::var::InferenceVariable {
    fn to_ty(self) -> ir::Ty {
        self.to_ty()
    }

    fn to_lifetime(self) -> ir::Lifetime {
        self.to_lifetime()
    }
}

crate mod prelude {
    #![allow(unused_imports)] // rustc bug

    crate use super::Context;
    crate use super::InferenceTable;
    crate use super::InferenceVariable;
    crate use super::UnificationResult;
}
