use crate::interner::ChalkIr;
use chalk_ir::cast::Cast;
use chalk_ir::interner::HasInterner;
use chalk_ir::{
    self, AdtId, AssocTypeId, BoundVar, ClosureId, DebruijnIndex, FnDefId, ImplId, OpaqueTyId,
    TraitId, VariableKinds,
};
use chalk_parse::ast::*;
use chalk_solve::rust_ir::{Anonymize, AssociatedTyValueId};
use std::collections::{BTreeMap, HashSet};

use super::{LowerParameterMap, LowerTypeKind};
use crate::error::RustIrError;
use crate::{Identifier as Ident, RawId, TypeKind};

pub type AdtIds = BTreeMap<Ident, chalk_ir::AdtId<ChalkIr>>;
pub type FnDefIds = BTreeMap<Ident, chalk_ir::FnDefId<ChalkIr>>;
pub type ClosureIds = BTreeMap<Ident, chalk_ir::ClosureId<ChalkIr>>;
pub type TraitIds = BTreeMap<Ident, chalk_ir::TraitId<ChalkIr>>;
pub type OpaqueTyIds = BTreeMap<Ident, chalk_ir::OpaqueTyId<ChalkIr>>;
pub type AdtKinds = BTreeMap<chalk_ir::AdtId<ChalkIr>, TypeKind>;
pub type FnDefKinds = BTreeMap<chalk_ir::FnDefId<ChalkIr>, TypeKind>;
pub type ClosureKinds = BTreeMap<chalk_ir::ClosureId<ChalkIr>, TypeKind>;
pub type TraitKinds = BTreeMap<chalk_ir::TraitId<ChalkIr>, TypeKind>;
pub type AutoTraits = BTreeMap<chalk_ir::TraitId<ChalkIr>, bool>;
pub type OpaqueTyKinds = BTreeMap<chalk_ir::OpaqueTyId<ChalkIr>, TypeKind>;
pub type AssociatedTyLookups = BTreeMap<(chalk_ir::TraitId<ChalkIr>, Ident), AssociatedTyLookup>;
pub type AssociatedTyValueIds =
    BTreeMap<(chalk_ir::ImplId<ChalkIr>, Ident), AssociatedTyValueId<ChalkIr>>;
pub type ForeignIds = BTreeMap<Ident, chalk_ir::ForeignDefId<ChalkIr>>;

pub type ParameterMap = BTreeMap<Ident, chalk_ir::WithKind<ChalkIr, BoundVar>>;

pub type LowerResult<T> = Result<T, RustIrError>;

#[derive(Clone, Debug)]
pub struct Env<'k> {
    pub adt_ids: &'k AdtIds,
    pub adt_kinds: &'k AdtKinds,
    pub fn_def_ids: &'k FnDefIds,
    pub fn_def_kinds: &'k FnDefKinds,
    pub closure_ids: &'k ClosureIds,
    pub closure_kinds: &'k ClosureKinds,
    pub trait_ids: &'k TraitIds,
    pub trait_kinds: &'k TraitKinds,
    pub opaque_ty_ids: &'k OpaqueTyIds,
    pub opaque_ty_kinds: &'k OpaqueTyKinds,
    pub associated_ty_lookups: &'k AssociatedTyLookups,
    pub auto_traits: &'k AutoTraits,
    pub foreign_ty_ids: &'k ForeignIds,
    /// GenericArg identifiers are used as keys, therefore
    /// all identifiers in an environment must be unique (no shadowing).
    pub parameter_map: ParameterMap,
}

/// Information about an associated type **declaration** (i.e., an
/// `AssociatedTyDatum`). This information is gathered in the first
/// phase of creating the Rust IR and is then later used to lookup the
/// "id" of an associated type.
///
/// ```ignore
/// trait Foo {
///     type Bar<'a>; // <-- associated type declaration
///          // ----
///          // |
///          // addl_variable_kinds
/// }
/// ```
#[derive(Debug, PartialEq, Eq)]
pub struct AssociatedTyLookup {
    pub id: chalk_ir::AssocTypeId<ChalkIr>,
    pub addl_variable_kinds: Vec<chalk_ir::VariableKind<ChalkIr>>,
}

pub enum ApplyTypeLookup<'k> {
    Parameter(&'k chalk_ir::WithKind<ChalkIr, BoundVar>),
    Adt(AdtId<ChalkIr>),
    FnDef(FnDefId<ChalkIr>),
    Closure(ClosureId<ChalkIr>),
    Opaque(OpaqueTyId<ChalkIr>),
}

#[derive(Default)]
pub struct ProgramLowerer {
    pub adt_ids: AdtIds,
    pub fn_def_ids: FnDefIds,
    pub closure_ids: ClosureIds,
    pub trait_ids: TraitIds,
    pub auto_traits: AutoTraits,
    pub opaque_ty_ids: OpaqueTyIds,
    pub associated_ty_lookups: AssociatedTyLookups,
    pub associated_ty_value_ids: AssociatedTyValueIds,
    pub adt_kinds: AdtKinds,
    pub fn_def_kinds: FnDefKinds,
    pub closure_kinds: ClosureKinds,
    pub trait_kinds: TraitKinds,
    pub opaque_ty_kinds: OpaqueTyKinds,
    pub foreign_ty_ids: ForeignIds,
    pub object_safe_traits: HashSet<TraitId<ChalkIr>>,
}

impl ProgramLowerer {
    pub fn gather_ids(&mut self, program: &Program) -> LowerResult<Vec<RawId>> {
        let mut index = 0;
        let mut next_item_id = || -> RawId {
            let i = index;
            index += 1;
            RawId { index: i }
        };

        // Make a vector mapping each thing in `items` to an id,
        // based just on its position:
        let raw_ids: Vec<_> = program.items.iter().map(|_| next_item_id()).collect();

        for (item, raw_id) in program.items.iter().zip(&raw_ids) {
            match item {
                Item::TraitDefn(d) => {
                    if d.flags.auto && !d.assoc_ty_defns.is_empty() {
                        Err(RustIrError::AutoTraitAssociatedTypes(d.name.clone()))?;
                    }
                    for defn in &d.assoc_ty_defns {
                        let addl_variable_kinds = defn.all_parameters();
                        let lookup = AssociatedTyLookup {
                            id: AssocTypeId(next_item_id()),
                            addl_variable_kinds: addl_variable_kinds.anonymize(),
                        };
                        self.associated_ty_lookups
                            .insert((TraitId(*raw_id), defn.name.str.clone()), lookup);
                    }
                }

                Item::Impl(d) => {
                    for atv in &d.assoc_ty_values {
                        let atv_id = AssociatedTyValueId(next_item_id());
                        self.associated_ty_value_ids
                            .insert((ImplId(*raw_id), atv.name.str.clone()), atv_id);
                    }
                }

                _ => {}
            }
        }

        for (item, raw_id) in program.items.iter().zip(&raw_ids) {
            match item {
                Item::AdtDefn(defn) => {
                    let type_kind = defn.lower_type_kind()?;
                    let id = AdtId(*raw_id);
                    self.adt_ids.insert(type_kind.name.clone(), id);
                    self.adt_kinds.insert(id, type_kind);
                }
                Item::FnDefn(defn) => {
                    let type_kind = defn.lower_type_kind()?;
                    let id = FnDefId(*raw_id);
                    self.fn_def_ids.insert(type_kind.name.clone(), id);
                    self.fn_def_kinds.insert(id, type_kind);
                }
                Item::ClosureDefn(defn) => {
                    let type_kind = defn.lower_type_kind()?;
                    let id = ClosureId(*raw_id);
                    self.closure_ids.insert(defn.name.str.clone(), id);
                    self.closure_kinds.insert(id, type_kind);
                }
                Item::TraitDefn(defn) => {
                    let type_kind = defn.lower_type_kind()?;
                    let id = TraitId(*raw_id);
                    self.trait_ids.insert(type_kind.name.clone(), id);
                    self.trait_kinds.insert(id, type_kind);
                    self.auto_traits.insert(id, defn.flags.auto);

                    if defn.flags.object_safe {
                        self.object_safe_traits.insert(id);
                    }
                }
                Item::OpaqueTyDefn(defn) => {
                    let type_kind = defn.lower_type_kind()?;
                    let id = OpaqueTyId(*raw_id);
                    self.opaque_ty_ids.insert(defn.name.str.clone(), id);
                    self.opaque_ty_kinds.insert(id, type_kind);
                }
                Item::Impl(_) => continue,
                Item::Clause(_) => continue,
                Item::Foreign(_) => continue,
            };
        }

        Ok(raw_ids)
    }
}

impl<'k> Env<'k> {
    pub fn interner(&self) -> &ChalkIr {
        &ChalkIr
    }

    pub fn from_lowerer(lowerer: &'k ProgramLowerer) -> Self {
        Self {
            adt_ids: &lowerer.adt_ids,
            adt_kinds: &lowerer.adt_kinds,
            fn_def_ids: &lowerer.fn_def_ids,
            fn_def_kinds: &lowerer.fn_def_kinds,
            closure_ids: &lowerer.closure_ids,
            closure_kinds: &lowerer.closure_kinds,
            trait_ids: &lowerer.trait_ids,
            trait_kinds: &lowerer.trait_kinds,
            opaque_ty_ids: &lowerer.opaque_ty_ids,
            opaque_ty_kinds: &lowerer.opaque_ty_kinds,
            associated_ty_lookups: &lowerer.associated_ty_lookups,
            parameter_map: BTreeMap::new(),
            auto_traits: &lowerer.auto_traits,
            foreign_ty_ids: &lowerer.foreign_ty_ids,
        }
    }

    pub fn lookup_generic_arg(
        &self,
        name: &Identifier,
    ) -> LowerResult<chalk_ir::GenericArg<ChalkIr>> {
        let interner = self.interner();

        let apply = |k: &TypeKind, type_name: chalk_ir::TypeName<ChalkIr>| {
            if k.binders.len(interner) > 0 {
                Err(RustIrError::IncorrectNumberOfTypeParameters {
                    identifier: name.clone(),
                    expected: k.binders.len(interner),
                    actual: 0,
                })
            } else {
                Ok(chalk_ir::TyData::Apply(chalk_ir::ApplicationTy {
                    name: type_name,
                    substitution: chalk_ir::Substitution::empty(interner),
                })
                .intern(interner)
                .cast(interner))
            }
        };

        match self.lookup_apply_type(name) {
            Ok(ApplyTypeLookup::Parameter(p)) => {
                let b = p.skip_kind();
                Ok(match &p.kind {
                    chalk_ir::VariableKind::Ty(_) => chalk_ir::TyData::BoundVar(*b)
                        .intern(interner)
                        .cast(interner),
                    chalk_ir::VariableKind::Lifetime => chalk_ir::LifetimeData::BoundVar(*b)
                        .intern(interner)
                        .cast(interner),
                    chalk_ir::VariableKind::Const(ty) => {
                        b.to_const(interner, ty.clone()).cast(interner)
                    }
                })
            }
            Ok(ApplyTypeLookup::Adt(id)) => apply(self.adt_kind(id), chalk_ir::TypeName::Adt(id)),
            Ok(ApplyTypeLookup::FnDef(id)) => {
                apply(self.fn_def_kind(id), chalk_ir::TypeName::FnDef(id))
            }
            Ok(ApplyTypeLookup::Closure(id)) => {
                apply(self.closure_kind(id), chalk_ir::TypeName::Closure(id))
            }
            Ok(ApplyTypeLookup::Opaque(id)) => Ok(chalk_ir::TyData::Alias(
                chalk_ir::AliasTy::Opaque(chalk_ir::OpaqueTy {
                    opaque_ty_id: id,
                    substitution: chalk_ir::Substitution::empty(interner),
                }),
            )
            .intern(interner)
            .cast(interner)),
            Err(_) => {
                if let Some(id) = self.foreign_ty_ids.get(&name.str) {
                    Ok(chalk_ir::TyData::Apply(chalk_ir::ApplicationTy {
                        name: chalk_ir::TypeName::Foreign(*id),
                        substitution: chalk_ir::Substitution::empty(interner),
                    })
                    .intern(interner)
                    .cast(interner))
                } else if let Some(_) = self.trait_ids.get(&name.str) {
                    Err(RustIrError::NotStruct(name.clone()))
                } else {
                    Err(RustIrError::InvalidParameterName(name.clone()))
                }
            }
        }
    }

    pub fn lookup_apply_type(&self, name: &Identifier) -> LowerResult<ApplyTypeLookup> {
        if let Some(id) = self.parameter_map.get(&name.str) {
            Ok(ApplyTypeLookup::Parameter(id))
        } else if let Some(id) = self.adt_ids.get(&name.str) {
            Ok(ApplyTypeLookup::Adt(*id))
        } else if let Some(id) = self.fn_def_ids.get(&name.str) {
            Ok(ApplyTypeLookup::FnDef(*id))
        } else if let Some(id) = self.closure_ids.get(&name.str) {
            Ok(ApplyTypeLookup::Closure(*id))
        } else if let Some(id) = self.opaque_ty_ids.get(&name.str) {
            Ok(ApplyTypeLookup::Opaque(*id))
        } else {
            Err(RustIrError::NotStruct(name.clone()))
        }
    }

    pub fn auto_trait(&self, id: chalk_ir::TraitId<ChalkIr>) -> bool {
        self.auto_traits[&id]
    }

    pub fn lookup_trait(&self, name: &Identifier) -> LowerResult<TraitId<ChalkIr>> {
        if let Some(_) = self.parameter_map.get(&name.str) {
            Err(RustIrError::NotTrait(name.clone()))
        } else if let Some(_) = self.adt_ids.get(&name.str) {
            Err(RustIrError::NotTrait(name.clone()))
        } else if let Some(id) = self.trait_ids.get(&name.str) {
            Ok(*id)
        } else {
            Err(RustIrError::InvalidTraitName(name.clone()))
        }
    }

    pub fn trait_kind(&self, id: chalk_ir::TraitId<ChalkIr>) -> &TypeKind {
        &self.trait_kinds[&id]
    }

    pub fn adt_kind(&self, id: chalk_ir::AdtId<ChalkIr>) -> &TypeKind {
        &self.adt_kinds[&id]
    }

    pub fn fn_def_kind(&self, id: chalk_ir::FnDefId<ChalkIr>) -> &TypeKind {
        &self.fn_def_kinds[&id]
    }

    pub fn closure_kind(&self, id: chalk_ir::ClosureId<ChalkIr>) -> &TypeKind {
        &self.closure_kinds[&id]
    }

    pub fn opaque_kind(&self, id: chalk_ir::OpaqueTyId<ChalkIr>) -> &TypeKind {
        &self.opaque_ty_kinds[&id]
    }

    pub fn lookup_associated_ty(
        &self,
        trait_id: TraitId<ChalkIr>,
        ident: &Identifier,
    ) -> LowerResult<&AssociatedTyLookup> {
        self.associated_ty_lookups
            .get(&(trait_id, ident.str.clone()))
            .ok_or(RustIrError::MissingAssociatedType(ident.clone()))
    }

    /// Introduces new parameters, shifting the indices of existing
    /// parameters to accommodate them. The indices of the new binders
    /// will be assigned in order as they are iterated.
    pub fn introduce<I>(&self, binders: I) -> LowerResult<Self>
    where
        I: IntoIterator<Item = chalk_ir::WithKind<ChalkIr, Ident>>,
        I::IntoIter: ExactSizeIterator,
    {
        // As binders to introduce we recieve `ParameterKind<Ident>`,
        // which we need to transform into `(Ident, ParameterKind<BoundVar>)`,
        // because that is the key-value pair for ParameterMap.
        // `swap_inner` lets us do precisely that, replacing `Ident` inside
        // `ParameterKind<Ident>` with a `BoundVar` and returning both.
        let binders = binders.into_iter().enumerate().map(|(i, k)| {
            let (kind, name) = k.into();
            (
                name,
                chalk_ir::WithKind::new(kind, BoundVar::new(DebruijnIndex::INNERMOST, i)),
            )
        });
        let len = binders.len();

        // For things already in the parameter map, we take each existing key-value pair
        // `(Ident, ParameterKind<BoundVar>)` and shift in the inner `BoundVar`.
        let parameter_map: ParameterMap = self
            .parameter_map
            .iter()
            .map(|(k, v)| (k.clone(), v.map_ref(|b| b.shifted_in())))
            .chain(binders)
            .collect();
        if parameter_map.len() != self.parameter_map.len() + len {
            Err(RustIrError::DuplicateOrShadowedParameters)?;
        }
        Ok(Env {
            parameter_map,
            ..*self
        })
    }

    pub fn in_binders<I, T, OP>(&self, binders: I, op: OP) -> LowerResult<chalk_ir::Binders<T>>
    where
        I: IntoIterator<Item = chalk_ir::WithKind<ChalkIr, Ident>>,
        I::IntoIter: ExactSizeIterator,
        T: HasInterner<Interner = ChalkIr>,
        OP: FnOnce(&Self) -> LowerResult<T>,
    {
        let binders: Vec<_> = binders.into_iter().collect();
        let env = self.introduce(binders.iter().cloned())?;
        Ok(chalk_ir::Binders::new(
            VariableKinds::from_iter(self.interner(), binders.iter().map(|v| v.kind.clone())),
            op(&env)?,
        ))
    }
}
