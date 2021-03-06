use crate::TypeCheckDatabase;
use crate::TypeCheckResults;
use crate::TypeChecker;
use crate::UniverseBinder;
use generational_arena::Arena;
use indices::IndexVec;
use map::FxIndexMap;
use mir::DefId;
use ty::base_inferred::BaseInferred;
use ty::base_only::BaseOnly;
use ty::interners::TyInternTables;
use unify::InferVar;
use unify::UnificationTable;

crate fn base_type_check(
    db: &impl TypeCheckDatabase,
    fn_def_id: DefId,
) -> TypeCheckResults<BaseInferred> {
    let fn_body = db.fn_body(fn_def_id);
    let interners: &TyInternTables = db.intern_tables();
    let mut base_type_checker: TypeChecker<'_, _, BaseOnly> = TypeChecker {
        db,
        fn_def_id,
        hir: fn_body,
        ops_arena: Arena::new(),
        ops_blocked: FxIndexMap::default(),
        unify: UnificationTable::new(interners.clone()),
        results: TypeCheckResults::default(),
        universe_binders: IndexVec::from(vec![UniverseBinder::Root]),
    };
    base_type_checker.check_fn_body();

    loop {
        let vars: Vec<InferVar> = base_type_checker.unify.drain_events().collect();
        if vars.is_empty() {
            break;
        }
        for var in vars {
            base_type_checker.trigger_ops(var);
        }
    }

    unimplemented!()
}
