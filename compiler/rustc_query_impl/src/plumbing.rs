//! The implementation of the query system itself. This defines the macros that
//! generate the actual methods on tcx which find and execute the provider,
//! manage the caches, and so forth.

use crate::on_disk_cache::{CacheDecoder, CacheEncoder, EncodedDepNodeIndex};
use crate::profiling_support::QueryKeyStringCache;
use crate::QueryStates;
use crate::{on_disk_cache, QueryToConfig};
use rustc_data_structures::stable_hasher::{HashStable, StableHasher};
use rustc_data_structures::sync::{AtomicU64, Lock};
use rustc_errors::{Diagnostic, Handler};
use rustc_middle::dep_graph::{
    self, DepKind, DepKindStruct, DepNode, DepNodeIndex, SerializedDepNodeIndex,
};
use rustc_middle::query::Key;
use rustc_middle::ty::query::QueryEngine;
use rustc_middle::ty::tls::{self, ImplicitCtxt};
use rustc_middle::ty::{self, TyCtxt};
use rustc_query_system::dep_graph::{DepNodeParams, HasDepContext};
use rustc_query_system::ich::StableHashingContext;
use rustc_query_system::query::{
    force_query, QueryCache, QueryConfig, QueryContext, QueryJobId, QueryMap, QuerySideEffects,
    QueryStackFrame,
};
use rustc_query_system::{LayoutOfDepth, QueryOverflow};
use rustc_serialize::Decodable;
use rustc_session::Limit;
use rustc_span::def_id::LOCAL_CRATE;
use std::any::Any;
use std::num::NonZeroU64;
use thin_vec::ThinVec;

#[derive(Copy, Clone)]
pub struct QueryCtxt<'tcx> {
    pub tcx: TyCtxt<'tcx>,
    pub queries: &'tcx Queries<'tcx>,
}

impl<'tcx> std::ops::Deref for QueryCtxt<'tcx> {
    type Target = TyCtxt<'tcx>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.tcx
    }
}

impl<'tcx> HasDepContext for QueryCtxt<'tcx> {
    type DepKind = rustc_middle::dep_graph::DepKind;
    type DepContext = TyCtxt<'tcx>;

    #[inline]
    fn dep_context(&self) -> &Self::DepContext {
        &self.tcx
    }
}

impl QueryContext for QueryCtxt<'_> {
    fn next_job_id(self) -> QueryJobId {
        QueryJobId(
            NonZeroU64::new(
                self.queries.jobs.fetch_add(1, rustc_data_structures::sync::Ordering::Relaxed),
            )
            .unwrap(),
        )
    }

    fn current_query_job(self) -> Option<QueryJobId> {
        tls::with_related_context(*self, |icx| icx.query)
    }

    fn try_collect_active_jobs(self) -> Option<QueryMap<DepKind>> {
        self.queries.try_collect_active_jobs(*self)
    }

    // Interactions with on_disk_cache
    fn load_side_effects(self, prev_dep_node_index: SerializedDepNodeIndex) -> QuerySideEffects {
        self.queries
            .on_disk_cache
            .as_ref()
            .map(|c| c.load_side_effects(*self, prev_dep_node_index))
            .unwrap_or_default()
    }

    fn store_side_effects(self, dep_node_index: DepNodeIndex, side_effects: QuerySideEffects) {
        if let Some(c) = self.queries.on_disk_cache.as_ref() {
            c.store_side_effects(dep_node_index, side_effects)
        }
    }

    fn store_side_effects_for_anon_node(
        self,
        dep_node_index: DepNodeIndex,
        side_effects: QuerySideEffects,
    ) {
        if let Some(c) = self.queries.on_disk_cache.as_ref() {
            c.store_side_effects_for_anon_node(dep_node_index, side_effects)
        }
    }

    /// Executes a job by changing the `ImplicitCtxt` to point to the
    /// new query job while it executes. It returns the diagnostics
    /// captured during execution and the actual result.
    #[inline(always)]
    fn start_query<R>(
        self,
        token: QueryJobId,
        depth_limit: bool,
        diagnostics: Option<&Lock<ThinVec<Diagnostic>>>,
        compute: impl FnOnce() -> R,
    ) -> R {
        // The `TyCtxt` stored in TLS has the same global interner lifetime
        // as `self`, so we use `with_related_context` to relate the 'tcx lifetimes
        // when accessing the `ImplicitCtxt`.
        tls::with_related_context(*self, move |current_icx| {
            if depth_limit && !self.recursion_limit().value_within_limit(current_icx.query_depth) {
                self.depth_limit_error(token);
            }

            // Update the `ImplicitCtxt` to point to our new query job.
            let new_icx = ImplicitCtxt {
                tcx: *self,
                query: Some(token),
                diagnostics,
                query_depth: current_icx.query_depth + depth_limit as usize,
                task_deps: current_icx.task_deps,
            };

            // Use the `ImplicitCtxt` while we execute the query.
            tls::enter_context(&new_icx, compute)
        })
    }

    fn depth_limit_error(self, job: QueryJobId) {
        let mut span = None;
        let mut layout_of_depth = None;
        if let Some(map) = self.try_collect_active_jobs() {
            if let Some((info, depth)) = job.try_find_layout_root(map) {
                span = Some(info.job.span);
                layout_of_depth = Some(LayoutOfDepth { desc: info.query.description, depth });
            }
        }

        let suggested_limit = match self.recursion_limit() {
            Limit(0) => Limit(2),
            limit => limit * 2,
        };

        self.sess.emit_fatal(QueryOverflow {
            span,
            layout_of_depth,
            suggested_limit,
            crate_name: self.crate_name(LOCAL_CRATE),
        });
    }
}

impl<'tcx> QueryCtxt<'tcx> {
    #[inline]
    pub fn from_tcx(tcx: TyCtxt<'tcx>) -> Self {
        let queries = tcx.queries.as_any();
        let queries = unsafe {
            let queries = std::mem::transmute::<&dyn Any, &dyn Any>(queries);
            let queries = queries.downcast_ref().unwrap();
            let queries = std::mem::transmute::<&Queries<'_>, &Queries<'_>>(queries);
            queries
        };
        QueryCtxt { tcx, queries }
    }

    pub(crate) fn on_disk_cache(self) -> Option<&'tcx on_disk_cache::OnDiskCache<'tcx>> {
        self.queries.on_disk_cache.as_ref()
    }

    pub(super) fn encode_query_results(
        self,
        encoder: &mut CacheEncoder<'_, 'tcx>,
        query_result_index: &mut EncodedDepNodeIndex,
    ) {
        for query in &self.queries.query_structs {
            if let Some(encode) = query.encode_query_results {
                encode(self, encoder, query_result_index);
            }
        }
    }

    pub fn try_print_query_stack(
        self,
        query: Option<QueryJobId>,
        handler: &Handler,
        num_frames: Option<usize>,
    ) -> usize {
        rustc_query_system::query::print_query_stack(self, query, handler, num_frames)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct QueryStruct<'tcx> {
    pub try_collect_active_jobs: fn(QueryCtxt<'tcx>, &mut QueryMap<DepKind>) -> Option<()>,
    pub alloc_self_profile_query_strings: fn(TyCtxt<'tcx>, &mut QueryKeyStringCache),
    pub encode_query_results:
        Option<fn(QueryCtxt<'tcx>, &mut CacheEncoder<'_, 'tcx>, &mut EncodedDepNodeIndex)>,
}

macro_rules! handle_cycle_error {
    ([]) => {{
        rustc_query_system::HandleCycleError::Error
    }};
    ([(fatal_cycle) $($rest:tt)*]) => {{
        rustc_query_system::HandleCycleError::Fatal
    }};
    ([(cycle_delay_bug) $($rest:tt)*]) => {{
        rustc_query_system::HandleCycleError::DelayBug
    }};
    ([$other:tt $($modifiers:tt)*]) => {
        handle_cycle_error!([$($modifiers)*])
    };
}

macro_rules! is_anon {
    ([]) => {{
        false
    }};
    ([(anon) $($rest:tt)*]) => {{
        true
    }};
    ([$other:tt $($modifiers:tt)*]) => {
        is_anon!([$($modifiers)*])
    };
}

macro_rules! is_eval_always {
    ([]) => {{
        false
    }};
    ([(eval_always) $($rest:tt)*]) => {{
        true
    }};
    ([$other:tt $($modifiers:tt)*]) => {
        is_eval_always!([$($modifiers)*])
    };
}

macro_rules! depth_limit_type {
    ([]) => {
        False
    };
    ([(depth_limit) $($rest:tt)*]) => {
        True
    };
    ([$other:tt $($modifiers:tt)*]) => {
        depth_limit_type!([$($modifiers)*])
    };
}

macro_rules! feedable_type {
    ([]) => {
        False
    };
    ([(feedable) $($rest:tt)*]) => {
        True
    };
    ([$other:tt $($modifiers:tt)*]) => {
        feedable_type!([$($modifiers)*])
    };
}

macro_rules! is_anon_type {
    ([]) => {
        False
    };
    ([(anon) $($rest:tt)*]) => {
        True
    };
    ([$other:tt $($modifiers:tt)*]) => {
        is_anon_type!([$($modifiers)*])
    };
}

macro_rules! if_arena {
    ([] $arena:tt $no_arena:tt) => {
        $no_arena
    };
    ([(arena_cache) $($rest:tt)*] $arena:tt $no_arena:tt) => {
        $arena
    };
    ([$other:tt $($modifiers:tt)*]$($args:tt)*) => {
        if_arena!([$($modifiers)*]$($args)*)
    };
}

macro_rules! if_extern_provider {
    ([] $yes:tt $no:tt) => {
        $no
    };
    ([(separate_provide_extern) $($rest:tt)*] $yes:tt $no:tt) => {
        $yes
    };
    ([$other:tt $($modifiers:tt)*]$($args:tt)*) => {
        if_extern_provider!([$($modifiers)*]$($args)*)
    };
}

macro_rules! if_compute_is_local_provider {
    ($modifiers:tt $yes:tt $no:tt) => {
        if_extern_provider!($modifiers $no (
            if_arena!($modifiers $no $yes)
        ))
    };
}

macro_rules! hash_result {
    ([][$V:ty]) => {{
        Some(|hcx, result| dep_graph::hash_result(hcx, result))
    }};
    ([(no_hash) $($rest:tt)*][$V:ty]) => {{
        None
    }};
    ([$other:tt $($modifiers:tt)*][$($args:tt)*]) => {
        hash_result!([$($modifiers)*][$($args)*])
    };
}

macro_rules! get_provider {
    ([][$tcx:expr, $name:ident, $key:expr]) => {{
        $tcx.query_system.local_providers.$name
    }};
    ([(separate_provide_extern) $($rest:tt)*][$tcx:expr, $name:ident, $key:expr]) => {{
        if $key.query_crate_is_local() {
            $tcx.query_system.local_providers.$name
        } else {
            $tcx.query_system.extern_providers.$name
        }
    }};
    ([$other:tt $($modifiers:tt)*][$($args:tt)*]) => {
        get_provider!([$($modifiers)*][$($args)*])
    };
}

macro_rules! should_ever_cache_on_disk {
    ([]$yes:tt $no:tt) => {{
        $no
    }};
    ([(cache) $($rest:tt)*]$yes:tt $no:tt) => {{
        $yes
    }};
    ([$other:tt $($modifiers:tt)*]$yes:tt $no:tt) => {
        should_ever_cache_on_disk!([$($modifiers)*]$yes $no)
    };
}

pub(crate) fn create_query_frame<
    'tcx,
    K: Copy + Key + for<'a> HashStable<StableHashingContext<'a>>,
>(
    tcx: QueryCtxt<'tcx>,
    do_describe: fn(TyCtxt<'tcx>, K) -> String,
    key: K,
    kind: DepKind,
    name: &'static str,
) -> QueryStackFrame<DepKind> {
    // Avoid calling queries while formatting the description
    let description = ty::print::with_no_queries!(
        // Disable visible paths printing for performance reasons.
        // Showing visible path instead of any path is not that important in production.
        ty::print::with_no_visible_paths!(
            // Force filename-line mode to avoid invoking `type_of` query.
            ty::print::with_forced_impl_filename_line!(do_describe(tcx.tcx, key))
        )
    );
    let description =
        if tcx.sess.verbose() { format!("{description} [{name:?}]") } else { description };
    let span = if kind == dep_graph::DepKind::def_span {
        // The `def_span` query is used to calculate `default_span`,
        // so exit to avoid infinite recursion.
        None
    } else {
        Some(key.default_span(*tcx))
    };
    let def_id = key.key_as_def_id();
    let def_kind = if kind == dep_graph::DepKind::opt_def_kind {
        // Try to avoid infinite recursion.
        None
    } else {
        def_id.and_then(|def_id| def_id.as_local()).and_then(|def_id| tcx.opt_def_kind(def_id))
    };
    let hash = || {
        tcx.with_stable_hashing_context(|mut hcx| {
            let mut hasher = StableHasher::new();
            std::mem::discriminant(&kind).hash_stable(&mut hcx, &mut hasher);
            key.hash_stable(&mut hcx, &mut hasher);
            hasher.finish::<u64>()
        })
    };
    let ty_adt_id = key.ty_adt_id();

    QueryStackFrame::new(description, span, def_id, def_kind, kind, ty_adt_id, hash)
}

fn try_load_from_on_disk_cache<'tcx, Q>(query: Q, tcx: TyCtxt<'tcx>, dep_node: DepNode)
where
    Q: QueryConfig<QueryCtxt<'tcx>>,
{
    debug_assert!(tcx.dep_graph.is_green(&dep_node));

    let key = Q::Key::recover(tcx, &dep_node).unwrap_or_else(|| {
        panic!("Failed to recover key for {:?} with hash {}", dep_node, dep_node.hash)
    });
    if query.cache_on_disk(tcx, &key) {
        let _ = query.execute_query(tcx, key);
    }
}

pub(crate) fn try_load_from_disk<'tcx, V>(
    tcx: QueryCtxt<'tcx>,
    id: SerializedDepNodeIndex,
) -> Option<V>
where
    V: for<'a> Decodable<CacheDecoder<'a, 'tcx>>,
{
    tcx.on_disk_cache().as_ref()?.try_load_query_result(*tcx, id)
}

fn force_from_dep_node<'tcx, Q>(query: Q, tcx: TyCtxt<'tcx>, dep_node: DepNode) -> bool
where
    Q: QueryConfig<QueryCtxt<'tcx>>,
{
    // We must avoid ever having to call `force_from_dep_node()` for a
    // `DepNode::codegen_unit`:
    // Since we cannot reconstruct the query key of a `DepNode::codegen_unit`, we
    // would always end up having to evaluate the first caller of the
    // `codegen_unit` query that *is* reconstructible. This might very well be
    // the `compile_codegen_unit` query, thus re-codegenning the whole CGU just
    // to re-trigger calling the `codegen_unit` query with the right key. At
    // that point we would already have re-done all the work we are trying to
    // avoid doing in the first place.
    // The solution is simple: Just explicitly call the `codegen_unit` query for
    // each CGU, right after partitioning. This way `try_mark_green` will always
    // hit the cache instead of having to go through `force_from_dep_node`.
    // This assertion makes sure, we actually keep applying the solution above.
    debug_assert!(
        dep_node.kind != DepKind::codegen_unit,
        "calling force_from_dep_node() on DepKind::codegen_unit"
    );

    if let Some(key) = Q::Key::recover(tcx, &dep_node) {
        #[cfg(debug_assertions)]
        let _guard = tracing::span!(tracing::Level::TRACE, stringify!($name), ?key).entered();
        let tcx = QueryCtxt::from_tcx(tcx);
        force_query(query, tcx, key, dep_node);
        true
    } else {
        false
    }
}

pub(crate) fn query_callback<'tcx, Q>(is_anon: bool, is_eval_always: bool) -> DepKindStruct<'tcx>
where
    Q: QueryToConfig<'tcx>,
    for<'a> <Q::Cache as QueryCache>::Key: HashStable<StableHashingContext<'a>>,
{
    let fingerprint_style = Q::Key::fingerprint_style();

    if is_anon || !fingerprint_style.reconstructible() {
        return DepKindStruct {
            is_anon,
            is_eval_always,
            fingerprint_style,
            force_from_dep_node: None,
            try_load_from_on_disk_cache: None,
        };
    }

    DepKindStruct {
        is_anon,
        is_eval_always,
        fingerprint_style,
        force_from_dep_node: Some(|tcx, dep_node| {
            force_from_dep_node(Q::config(QueryCtxt::from_tcx(tcx)), tcx, dep_node)
        }),
        try_load_from_on_disk_cache: Some(|tcx, dep_node| {
            try_load_from_on_disk_cache(Q::config(QueryCtxt::from_tcx(tcx)), tcx, dep_node)
        }),
    }
}

macro_rules! expand_if_cached {
    ([], $tokens:expr) => {{
        None
    }};
    ([(cache) $($rest:tt)*], $tokens:expr) => {{
        Some($tokens)
    }};
    ([$other:tt $($modifiers:tt)*], $tokens:expr) => {
        expand_if_cached!([$($modifiers)*], $tokens)
    };
}

// NOTE: `$V` isn't used here, but we still need to match on it so it can be passed to other macros
// invoked by `rustc_query_append`.
macro_rules! define_queries {
    (
     $($(#[$attr:meta])*
        [$($modifiers:tt)*] fn $name:ident($($K:tt)*) -> $V:ty,)*) => {
        define_queries_struct! {
            input: ($(([$($modifiers)*] [$($attr)*] [$name]))*)
        }

        #[allow(nonstandard_style)]
        mod queries {
            use std::marker::PhantomData;

            $(
                #[derive(Copy, Clone, Default)]
                pub struct $name<'tcx> {
                    data: PhantomData<&'tcx ()>
                }
            )*
        }

        #[allow(nonstandard_style)]
        mod dynamic_query {
            use super::*;

            $(
                pub(super) fn $name<'tcx>(
                    _local_provider: fn(
                        tcx: TyCtxt<'tcx>, key: query_keys::$name<'tcx>
                    ) -> query_provided::$name<'tcx>
                ) -> DynamicQuery<'tcx, query_storage::$name<'tcx>> {
                    DynamicQuery {
                        name: stringify!($name),
                        cache_on_disk: |tcx, key| ::rustc_middle::query::cached::$name(tcx, key),
                        execute_query: |tcx, key| tcx.$name(key),
                        compute: if_compute_is_local_provider!(
                            [$($modifiers)*]
                            _local_provider
                            (|tcx, key: query_keys::$name<'tcx>| query_provided_to_value::$name(
                                tcx,
                                get_provider!([$($modifiers)*][tcx, $name, key])(tcx, key)
                            ))
                        ),
                        try_load_from_disk: |qcx, key| {
                            fn try_load_from_disk<'tcx>(
                                _qcx: QueryCtxt<'tcx>,
                                _key: &query_keys::$name<'tcx>
                            ) -> TryLoadFromDisk<QueryCtxt<'tcx>, query_values::$name<'tcx>> {
                                should_ever_cache_on_disk!([$($modifiers)*] {
                                    if ::rustc_middle::query::cached::$name(_qcx.tcx, _key) {
                                        Some(|qcx, dep_node| {
                                            let value = $crate::plumbing::try_load_from_disk::<query_provided::$name<'tcx>>(
                                                qcx,
                                                dep_node
                                            );
                                            value.map(|value| query_provided_to_value::$name(qcx.tcx, value))
                                        })
                                    } else {
                                        None
                                    }
                                } {
                                    None
                                })
                            }
                            try_load_from_disk(qcx, key)
                        },
                        query_state: offset_of!(QueryStates<'static>, $name),
                        query_cache: offset_of!(QueryCaches<'static>, $name),
                        eval_always: is_eval_always!([$($modifiers)*]),
                        dep_kind: dep_graph::DepKind::$name,
                        handle_cycle_error: handle_cycle_error!([$($modifiers)*]),
                        hash_result: hash_result!([$($modifiers)*][query_values::$name]),
                    }
                }
            )*
        }

        $(impl<'tcx> QueryToConfig<'tcx> for queries::$name<'tcx> {
            type Cache = query_storage::$name<'tcx>;
            type Key = query_keys::$name<'tcx>;

            type Anon = is_anon_type!([$($modifiers)*]);
            type DepthLimit = depth_limit_type!([$($modifiers)*]);
            type Feedable = feedable_type!([$($modifiers)*]);

            #[inline(always)]
            fn config(qcx: QueryCtxt<'tcx>) -> DynamicConfig<'tcx, Self::Cache, Self::Anon, Self::DepthLimit, Self::Feedable> {
                DynamicConfig {
                    dynamic: &qcx.queries.dynamic_queries.$name,
                    data: PhantomData,
                }
            }
        })*

        #[allow(nonstandard_style)]
        mod query_callbacks {
            use super::*;
            use rustc_query_system::dep_graph::FingerprintStyle;

            // We use this for most things when incr. comp. is turned off.
            pub fn Null<'tcx>() -> DepKindStruct<'tcx> {
                DepKindStruct {
                    is_anon: false,
                    is_eval_always: false,
                    fingerprint_style: FingerprintStyle::Unit,
                    force_from_dep_node: Some(|_, dep_node| bug!("force_from_dep_node: encountered {:?}", dep_node)),
                    try_load_from_on_disk_cache: None,
                }
            }

            // We use this for the forever-red node.
            pub fn Red<'tcx>() -> DepKindStruct<'tcx> {
                DepKindStruct {
                    is_anon: false,
                    is_eval_always: false,
                    fingerprint_style: FingerprintStyle::Unit,
                    force_from_dep_node: Some(|_, dep_node| bug!("force_from_dep_node: encountered {:?}", dep_node)),
                    try_load_from_on_disk_cache: None,
                }
            }

            pub fn TraitSelect<'tcx>() -> DepKindStruct<'tcx> {
                DepKindStruct {
                    is_anon: true,
                    is_eval_always: false,
                    fingerprint_style: FingerprintStyle::Unit,
                    force_from_dep_node: None,
                    try_load_from_on_disk_cache: None,
                }
            }

            pub fn CompileCodegenUnit<'tcx>() -> DepKindStruct<'tcx> {
                DepKindStruct {
                    is_anon: false,
                    is_eval_always: false,
                    fingerprint_style: FingerprintStyle::Opaque,
                    force_from_dep_node: None,
                    try_load_from_on_disk_cache: None,
                }
            }

            pub fn CompileMonoItem<'tcx>() -> DepKindStruct<'tcx> {
                DepKindStruct {
                    is_anon: false,
                    is_eval_always: false,
                    fingerprint_style: FingerprintStyle::Opaque,
                    force_from_dep_node: None,
                    try_load_from_on_disk_cache: None,
                }
            }

            $(pub(crate) fn $name<'tcx>()-> DepKindStruct<'tcx> {
                $crate::plumbing::query_callback::<queries::$name<'tcx>>(
                    is_anon!([$($modifiers)*]),
                    is_eval_always!([$($modifiers)*]),
                )
            })*
        }

        mod query_structs {
            use rustc_middle::ty::TyCtxt;
            use $crate::plumbing::{QueryStruct, QueryCtxt};
            use $crate::profiling_support::QueryKeyStringCache;
            use rustc_query_system::query::QueryMap;
            use rustc_middle::dep_graph::DepKind;
            use crate::QueryToConfig;

            pub(super) const fn dummy_query_struct<'tcx>() -> QueryStruct<'tcx> {
                fn noop_try_collect_active_jobs(_: QueryCtxt<'_>, _: &mut QueryMap<DepKind>) -> Option<()> {
                    None
                }
                fn noop_alloc_self_profile_query_strings(_: TyCtxt<'_>, _: &mut QueryKeyStringCache) {}

                QueryStruct {
                    try_collect_active_jobs: noop_try_collect_active_jobs,
                    alloc_self_profile_query_strings: noop_alloc_self_profile_query_strings,
                    encode_query_results: None,
                }
            }

            pub(super) use dummy_query_struct as Null;
            pub(super) use dummy_query_struct as Red;
            pub(super) use dummy_query_struct as TraitSelect;
            pub(super) use dummy_query_struct as CompileCodegenUnit;
            pub(super) use dummy_query_struct as CompileMonoItem;

            $(
            pub(super) const fn $name<'tcx>() -> QueryStruct<'tcx> { QueryStruct {
                try_collect_active_jobs: |tcx, qmap| {
                    let make_query = |tcx, key| {
                        let kind = rustc_middle::dep_graph::DepKind::$name;
                        let name = stringify!($name);
                        $crate::plumbing::create_query_frame(tcx, rustc_middle::query::descs::$name, key, kind, name)
                    };
                    tcx.queries.query_states.$name.try_collect_active_jobs(
                        tcx,
                        make_query,
                        qmap,
                    )
                },
                alloc_self_profile_query_strings: |tcx, string_cache| {
                    $crate::profiling_support::alloc_self_profile_query_strings_for_query_cache(
                        tcx,
                        stringify!($name),
                        &tcx.query_system.caches.$name,
                        string_cache,
                    )
                },
                encode_query_results: expand_if_cached!([$($modifiers)*], |qcx, encoder, query_result_index|
                    $crate::on_disk_cache::encode_query_results(
                        super::queries::$name::config(qcx),
                        qcx,
                        encoder,
                        query_result_index,
                    )
                ),
            }})*
        }

        pub fn query_callbacks<'tcx>(arena: &'tcx Arena<'tcx>) -> &'tcx [DepKindStruct<'tcx>] {
            arena.alloc_from_iter(make_dep_kind_array!(query_callbacks))
        }
    }
}

use crate::{OnDiskCache, Providers};

pub struct Queries<'tcx> {
    pub(crate) query_structs: Vec<QueryStruct<'tcx>>,
    pub on_disk_cache: Option<OnDiskCache<'tcx>>,
    jobs: AtomicU64,
    pub(crate) dynamic_queries: crate::DynamicQueries<'tcx>,
    pub(crate) query_states: crate::QueryStates<'tcx>,
}

impl<'tcx> Queries<'tcx> {
    pub fn new(local_providers: &Providers, on_disk_cache: Option<OnDiskCache<'tcx>>) -> Self {
        use crate::query_structs;
        Queries {
            dynamic_queries: crate::DynamicQueries::new(&local_providers),
            query_structs: make_dep_kind_array!(query_structs).to_vec(),
            on_disk_cache,
            jobs: AtomicU64::new(1),
            query_states: QueryStates::default(),
        }
    }

    // Force codegen in the dyn-trait transformation in this crate.
    pub fn as_dyn(&'tcx self) -> &'tcx dyn QueryEngine<'tcx> {
        self
    }

    pub(crate) fn try_collect_active_jobs(
        &'tcx self,
        tcx: TyCtxt<'tcx>,
    ) -> Option<QueryMap<rustc_middle::dep_graph::DepKind>> {
        let tcx = QueryCtxt { tcx, queries: self };
        let mut jobs = QueryMap::default();

        for query in &self.query_structs {
            (query.try_collect_active_jobs)(tcx, &mut jobs);
        }

        Some(jobs)
    }
}

macro_rules! define_queries_struct {
    (
     input: ($(([$($modifiers:tt)*] [$($attr:tt)*] [$name:ident]))*)) => {
        #[allow(dead_code)]
        struct DynamicQueries<'tcx> {
            $(
                $name: DynamicQuery<'tcx, query_storage::$name<'tcx>>,
            )*
        }

        impl DynamicQueries<'_> {
            fn new(local_providers: &Providers) -> Self {
                Self {
                    $(
                        $name: dynamic_query::$name(local_providers.$name),
                    )*
                }
            }
        }

        #[derive(Default)]
        struct QueryStates<'tcx> {
            $(
                $name: QueryState<query_keys::$name<'tcx>, rustc_middle::dep_graph::DepKind>,
            )*
        }

        impl<'tcx> QueryEngine<'tcx> for Queries<'tcx> {
            fn as_any(&'tcx self) -> &'tcx dyn std::any::Any {
                let this = unsafe { std::mem::transmute::<&Queries<'_>, &Queries<'_>>(self) };
                this as _
            }

            fn try_mark_green(&'tcx self, tcx: TyCtxt<'tcx>, dep_node: &dep_graph::DepNode) -> bool {
                let qcx = QueryCtxt { tcx, queries: self };
                tcx.dep_graph.try_mark_green(qcx, dep_node).is_some()
            }

            $($(#[$attr])*
            #[inline(always)]
            #[tracing::instrument(level = "trace", skip(self, tcx), ret)]
            fn $name(
                &'tcx self,
                tcx: TyCtxt<'tcx>,
                span: Span,
                key: query_keys::$name<'tcx>,
                mode: QueryMode,
            ) -> Option<query_values::$name<'tcx>> {
                let qcx = QueryCtxt { tcx, queries: self };
                get_query(
                    queries::$name::config(qcx),
                    qcx,
                    span,
                    key,
                    mode
                )
            })*
        }
    };
}
