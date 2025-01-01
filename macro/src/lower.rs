use std::collections::HashMap;
use std::ops::Deref;

use proc_macro2::Span;
use proc_macro_error::abort;

use syn::parse::Parser;
use syn::parse_quote;
use syn::{
    Expr, ExprCall, Field, FnArg, GenericParam, Generics, Ident, ItemFn, ItemImpl, Lifetime, Pat,
    PatType, Path, Type, Variant, Visibility, WhereClause, WherePredicate,
};

use quote::format_ident;

use crate::analyze;
use crate::analyze::Model;
use crate::visitors::{GenericParamVisitor, LifetimeVisitor};
use crate::SUPERSTATE_LIFETIME;

/// Intermediate representation of the state machine.
#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub struct Ir {
    /// A copy of the item impl that was parsed.
    pub item_impl: ItemImpl,
    /// General information regarding the staet machine.
    pub state_machine: StateMachine,
    /// The states of the state machine.
    pub states: HashMap<Ident, State>,
    /// The superstate of the state machine.
    pub superstates: HashMap<Ident, Superstate>,
}

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
/// General information regarding the state machine.
pub struct StateMachine {
    /// Initial state.
    pub initial_state: ExprCall,
    /// The type on which the state machine is implemented.
    pub shared_storage_type: Type,
    /// The generics associated with the shared storage type.
    pub shared_storage_generics: Generics,
    /// The type of the event.
    pub event_type: Type,
    /// The type of the context.
    pub context_type: Type,
    /// The type of the state enum.
    pub state_ident: Ident,
    /// Derives that will be applied on the state type.
    pub state_derives: Vec<Path>,
    /// The generics associated with the state type.
    pub state_generics: Generics,
    /// The type of the superstate enum (ex. `Superstate<'sub>`)
    pub superstate_ident: Ident,
    /// Derives that will be applied to the superstate type.
    pub superstate_derives: Vec<Path>,
    /// The generics associated with the superstate type.
    pub superstate_generics: Generics,
    /// The path of the `on_transition` callback.
    pub on_transition: Option<Path>,
    /// The path of the `on_transition_async` callback.
    pub on_transition_async: Option<Path>,
    /// The path of the `on_dispatch` callback.
    pub on_dispatch: Option<Path>,
    /// The visibility for the derived types,
    pub visibility: Visibility,
    /// The external input pattern.
    pub event_ident: Ident,
    /// The external input pattern.
    pub context_ident: Ident,
    /// Whether the state machine is sync (blocking) or async (awaitable).
    pub mode: Mode,
}

/// Information regarding a state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct State {
    /// The variant that will be part of the state enum
    /// (e.g. `On { led: bool }`)
    pub variant: Variant,
    /// The pattern that we'll use to match on the state enum.
    /// (e.g. `State::On { led }`)
    pub pat: Pat,
    /// The call to the state handler
    /// (e.g. `Blinky::on(shared_storage, led, input)`).
    pub handler_call: Expr,
    /// The call to the entry action of the state, if defined
    /// (e.g. `Blinky::enter_on(shared_storage, led)`, `{}`, ..).
    pub entry_action_call: Expr,
    /// The call to the exit action of the state, if defined
    /// (e.g. `Blinky::exit_on(shared_storage, led)`, `{}`, ..).
    pub exit_action_call: Expr,
    /// The pattern to create the superstate variant.
    /// (e.g. `Some(Superstate::Playing { led })`, `None`, ..).
    pub superstate_pat: Pat,
    /// The constructor to create the state
    /// (e.g. `const fn on(led: bool) -> Self { Self::On { led }}`).
    pub constructor: ItemFn,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Superstate {
    /// The variant that will be part of the superstate enum
    /// (e.g. `Playing { led: &'mut bool }`).
    pub variant: Variant,
    /// The pattern that we'll use to mactch on the superstate enum
    /// (e.g. `Superstate::Playing { led }`).
    pub pat: Pat,
    /// The call to the superstate handler
    /// (e.g. `Blinky::playing(shared_storage, led)`)
    pub handler_call: Expr,
    /// The call to the entry action of the superstate, if defined
    /// (e.g. `Blinky::enter_playing(shared_storage, led)`)
    pub entry_action_call: Expr,
    /// The call to the exit action of the superstate, if defined
    /// (e.g. `Blinky::exit_playing(shared_storage, led)`).
    pub exit_action_call: Expr,
    /// The pattern to create the superstate variant.
    /// (e.g. `Some(Superstate::Playing { led })`, `None`, ..).
    pub superstate_pat: Expr,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Action {
    /// The call to the action.
    /// (e.g. `Blinky::exit_off(shared_storage, led)`)
    pub handler_call: Expr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Awaitable,
    Blocking,
}

pub fn lower(model: &Model) -> Ir {
    let item_impl = model.item_impl.clone();
    let initial_state = model.state_machine.initial_state.clone();
    let state_ident = model.state_machine.state_ident.clone();
    let superstate_ident = model.state_machine.superstate_ident.clone();
    let on_transition = model.state_machine.on_transition.clone();

    let on_transition_async = model.state_machine.on_transition_async.clone();
    let on_dispatch = model.state_machine.on_dispatch.clone();
    let event_ident = model.state_machine.event_ident.clone();
    let context_ident = model.state_machine.context_ident.clone();
    let shared_storage_type = model.state_machine.shared_storage_type.clone();
    let shared_storage_generics = model.state_machine.shared_storage_generics.clone();
    let state_derives = model.state_machine.state_derives.clone();
    let superstate_derives = model.state_machine.superstate_derives.clone();
    let visibility = model.state_machine.visibility.clone();

    let mut superstate_lifetime: Option<Lifetime> = None;

    let mut states: HashMap<Ident, State> = model
        .states
        .iter()
        .map(|(key, value)| (key.clone(), lower_state(value, &model.state_machine)))
        .collect();

    let mut superstates: HashMap<Ident, Superstate> = model
        .superstates
        .iter()
        .inspect(|(_, value)| {
            if !value.state_inputs.is_empty() {
                let lifetime = Lifetime::new(SUPERSTATE_LIFETIME, Span::call_site());
                superstate_lifetime = Some(lifetime);
            }
        })
        .map(|(key, value)| (key.clone(), lower_superstate(value, &model.state_machine)))
        .collect();

    let actions: HashMap<Ident, Action> = model
        .actions
        .iter()
        .map(|(key, value)| (key.clone(), lower_action(value, &model.state_machine)))
        .collect();

    // Linking states to their superstates and entry/exit actions.
    for (key, state) in &mut states {
        if let Some(superstate) = model
            .states
            .get(key)
            .and_then(|state| state.superstate.as_ref())
        {
            match superstates.get(superstate) {
                Some(superstate) => {
                    let superstate_pat = &superstate.pat;
                    state.superstate_pat = parse_quote!(Some(#superstate_pat))
                }
                None => abort!(superstate, "superstate not found"),
            }
        }

        if let Some(entry_action) = model
            .states
            .get(key)
            .and_then(|state| state.entry_action.as_ref())
        {
            match actions.get(entry_action) {
                Some(action) => state.entry_action_call = action.handler_call.clone(),
                None => abort!(entry_action, "entry action not found"),
            }
        }

        if let Some(exit_action) = model
            .states
            .get(key)
            .and_then(|state| state.exit_action.as_ref())
        {
            match actions.get(exit_action) {
                Some(action) => state.exit_action_call = action.handler_call.clone(),
                None => abort!(exit_action, "exit action not found"),
            }
        }
    }

    // Linking superstates to superstates and entry/exit action.
    let superstates_clone = superstates.clone();
    for (key, superstate) in &mut superstates {
        if let Some(superstate_superstate) = model
            .superstates
            .get(key)
            .and_then(|state| state.superstate.as_ref())
        {
            match superstates_clone.get(superstate_superstate) {
                Some(superstate_superstate) => {
                    let superstate_superstate_pat = &superstate_superstate.pat;
                    superstate.superstate_pat = parse_quote!(Some(#superstate_superstate_pat))
                }
                None => abort!(superstate_superstate, "superstate not found"),
            }
        }

        if let Some(entry_action) = model
            .superstates
            .get(key)
            .and_then(|state| state.entry_action.as_ref())
        {
            match actions.get(entry_action) {
                Some(action) => superstate.entry_action_call = action.handler_call.clone(),
                None => abort!(entry_action, "action not found"),
            }
        }

        if let Some(exit_action) = model
            .superstates
            .get(key)
            .and_then(|state| state.exit_action.as_ref())
        {
            match actions.get(exit_action) {
                Some(action) => superstate.exit_action_call = action.handler_call.clone(),
                None => abort!(exit_action, "action not found"),
            }
        }
    }

    // Find event and/or context types and check whether there are any async functions.
    let mut mode = Mode::Blocking;
    let mut event_type = None;
    let mut context_type = None;

    for state in model.states.values() {
        if let Some(pat_type) = &state.event_arg {
            if let Pat::Ident(external_input_ident) = &*pat_type.pat {
                if model
                    .state_machine
                    .event_ident
                    .eq(&external_input_ident.ident)
                {
                    let ty = match &*pat_type.ty {
                        Type::Reference(reference) => reference.elem.deref().clone(),
                        _ => abort!(pat_type.ty, "event must be passed in as a reference"),
                    };
                    event_type = Some(ty);
                }
            }
        }
        if let Some(pat_type) = &state.context_arg {
            if let Pat::Ident(external_input_ident) = &*pat_type.pat {
                if model
                    .state_machine
                    .context_ident
                    .eq(&external_input_ident.ident)
                {
                    let ty = match &*pat_type.ty {
                        Type::Reference(reference) => reference.elem.deref().clone(),
                        _ => abort!(pat_type.ty, "context must be passed in as a reference"),
                    };
                    context_type = Some(ty);
                }
            }
        }
        if state.is_async {
            mode = Mode::Awaitable;
        }
    }

    for superstate in model.superstates.values() {
        if let Some(pat_type) = &superstate.event_arg {
            if let Pat::Ident(external_input_ident) = &*pat_type.pat {
                if model
                    .state_machine
                    .event_ident
                    .eq(&external_input_ident.ident)
                {
                    let ty = match &*pat_type.ty {
                        Type::Reference(reference) => reference.elem.deref().clone(),
                        _ => abort!(pat_type.ty, "event must be passed in as a reference"),
                    };
                    event_type = Some(ty);
                }
            }
        }
        if let Some(pat_type) = &superstate.context_arg {
            if let Pat::Ident(external_input_ident) = &*pat_type.pat {
                if model
                    .state_machine
                    .context_ident
                    .eq(&external_input_ident.ident)
                {
                    let ty = match &*pat_type.ty {
                        Type::Reference(reference) => reference.elem.deref().clone(),
                        _ => abort!(pat_type.ty, "context must be passed in as a reference"),
                    };
                    context_type = Some(ty);
                }
            }
        }
        if superstate.is_async {
            mode = Mode::Awaitable;
        }
    }

    for action in model.actions.values() {
        if action.is_async {
            mode = Mode::Awaitable;
        }
    }

    // Set the event type if it was found, otherwise set it to `()`.
    let mut event_type = match event_type {
        Some(event_type) => event_type,
        None => parse_quote!(()),
    };

    // Rename all the anonymous lifetimes in the event type.
    let mut lifetime_visitor = LifetimeVisitor::new("'event");
    lifetime_visitor.rename_type(&mut event_type);

    // Set the context type if it was found, otherwise set it to `()`.
    let mut context_type = match context_type {
        Some(context_type) => context_type,
        None => parse_quote!(()),
    };

    // Rename all the anonymous lifetimes in the context type.
    let mut lifetime_visitor = LifetimeVisitor::new("'context");
    lifetime_visitor.rename_type(&mut context_type);

    // Find the generics that need to be included on the state and superstate enums.
    let shared_storage_generics_map = map_generics(&shared_storage_generics);

    let mut visitor = GenericParamVisitor::new(&model.state_machine.shared_storage_generics);
    for state in model.states.values() {
        visitor.search(&state.state_inputs);
    }
    let state_generic_params = visitor.finish();

    let mut visitor = GenericParamVisitor::new(&model.state_machine.shared_storage_generics);
    for superstate in model.superstates.values() {
        visitor.search(&superstate.state_inputs);
    }
    let superstate_generic_params = visitor.finish();

    let mut state_generics = Generics::default();
    for (param, predicates) in &shared_storage_generics_map {
        if state_generic_params.contains(param) {
            state_generics.params.push(param.clone());
            match &mut state_generics.where_clause {
                Some(clause) => clause.predicates.extend(predicates.iter().cloned()),
                None => {
                    state_generics.where_clause = Some(WhereClause {
                        where_token: parse_quote!(where),
                        predicates: parse_quote!(#(#predicates),*),
                    })
                }
            }
        }
    }

    let mut superstate_generics = Generics::default();
    for (param, predicates) in &shared_storage_generics_map {
        if superstate_generic_params.contains(param) {
            superstate_generics.params.push(param.clone());
            match &mut superstate_generics.where_clause {
                Some(clause) => clause.predicates.extend(predicates.iter().cloned()),
                None => {
                    superstate_generics.where_clause = Some(WhereClause {
                        where_token: parse_quote!(where),
                        predicates: parse_quote!(#(#predicates),*),
                    })
                }
            }
        }
    }

    // If a lifetime is required it must be part of the superstate generics.
    if let Some(lifetime) = superstate_lifetime {
        superstate_generics
            .params
            .push(GenericParam::Lifetime(syn::LifetimeDef::new(lifetime)));
    }

    let state_machine = StateMachine {
        initial_state,
        shared_storage_type,
        shared_storage_generics,
        event_type,
        context_type,
        state_ident,
        state_derives,
        state_generics,
        superstate_ident,
        superstate_derives,
        superstate_generics,
        on_transition,
        on_transition_async,
        on_dispatch,
        visibility,
        event_ident,
        context_ident,
        mode,
    };

    Ir {
        state_machine,
        item_impl,
        states,
        superstates,
    }
}

pub fn lower_state(state: &analyze::State, state_machine: &analyze::StateMachine) -> State {
    let variant_name = snake_case_to_pascal_case(&state.handler_name);
    let state_handler_name = &state.handler_name;
    let shared_storage_path = &state_machine.shared_storage_path;
    let (_, shared_storage_type_generics, _) =
        &state_machine.shared_storage_generics.split_for_impl();
    let shared_storage_turbofish = shared_storage_type_generics.as_turbofish();
    let state_name = &state_machine.state_ident;

    let mut variant_fields: Vec<_> = state
        .state_inputs
        .iter()
        .map(fn_arg_to_state_field)
        .collect();

    for field in &state.local_storage {
        match variant_fields.iter_mut().find(|f| f.ident == field.ident) {
            Some(item) => {
                *item = field.clone();
            }
            None => variant_fields.push(field.clone()),
        }
    }

    let pat_fields: Vec<Ident> = variant_fields
        .iter()
        .map(|field| field.ident.as_ref().unwrap().clone())
        .collect();
    let handler_inputs: Vec<Ident> = state.inputs.iter().map(fn_arg_to_ident).collect();

    let variant = parse_quote!(#variant_name { #(#variant_fields),* });
    let pat = parse_quote!(#state_name::#variant_name { #(#pat_fields),*});
    let constructor = parse_quote!(const fn #state_handler_name ( #(#variant_fields),* ) -> Self { Self::#variant_name { #(#pat_fields),*} });

    let handler_call = match &state.is_async {
        true => {
            parse_quote!(#shared_storage_path #shared_storage_turbofish ::#state_handler_name(#(#handler_inputs),*).await)
        }
        false => {
            parse_quote!(#shared_storage_path #shared_storage_turbofish ::#state_handler_name(#(#handler_inputs),*))
        }
    };

    let entry_action_call = parse_quote!({});
    let exit_action_call = parse_quote!({});
    let superstate_pat = parse_quote!(None);

    State {
        variant,
        pat,
        constructor,
        handler_call,
        entry_action_call,
        exit_action_call,
        superstate_pat,
    }
}

pub fn lower_superstate(
    superstate: &analyze::Superstate,
    state_machine: &analyze::StateMachine,
) -> Superstate {
    let superstate_name = snake_case_to_pascal_case(&superstate.handler_name);
    let superstate_handler_name = &superstate.handler_name;
    let shared_storage_path = &state_machine.shared_storage_path;
    let (_, shared_storage_type_generics, _) =
        &state_machine.shared_storage_generics.split_for_impl();
    let shared_storage_turbofish = shared_storage_type_generics.as_turbofish();
    let superstate_type = &state_machine.superstate_ident;

    let mut variant_fields: Vec<_> = superstate
        .state_inputs
        .iter()
        .map(fn_arg_to_superstate_field)
        .collect();

    for field in &superstate.local_storage {
        match variant_fields.iter_mut().find(|f| f.ident == field.ident) {
            Some(item) => {
                *item = field.clone();
            }
            None => variant_fields.push(field.clone()),
        }
    }

    let pat_fields: Vec<Ident> = variant_fields
        .iter()
        .map(|field| field.ident.as_ref().unwrap().clone())
        .collect();
    let handler_inputs: Vec<Ident> = superstate.inputs.iter().map(fn_arg_to_ident).collect();

    let variant = parse_quote!(#superstate_name { #(#variant_fields),* });
    let pat = parse_quote!(#superstate_type::#superstate_name { #(#pat_fields),*});

    let handler_call = match &superstate.is_async {
        true => {
            parse_quote!(#shared_storage_path #shared_storage_turbofish ::#superstate_handler_name(#(#handler_inputs),*).await)
        }
        false => {
            parse_quote!(#shared_storage_path #shared_storage_turbofish ::#superstate_handler_name(#(#handler_inputs),*))
        }
    };

    let entry_action_call = parse_quote!({});
    let exit_action_call = parse_quote!({});
    let superstate_pat = parse_quote!(None);

    Superstate {
        variant,
        pat,
        handler_call,
        entry_action_call,
        exit_action_call,
        superstate_pat,
    }
}

pub fn lower_action(action: &analyze::Action, state_machine: &analyze::StateMachine) -> Action {
    let action_handler_name = &action.handler_name;
    let shared_storage_path = &state_machine.shared_storage_path;
    let (_, shared_storage_type_generics, _) =
        &state_machine.shared_storage_generics.split_for_impl();
    let shared_storage_turbofish = shared_storage_type_generics.as_turbofish();

    let mut call_inputs: Vec<Ident> = Vec::new();

    for input in &action.inputs {
        match input {
            FnArg::Receiver(_) => {
                call_inputs.insert(0, parse_quote!(shared_storage));
            }

            // Typed argument.
            FnArg::Typed(pat_type) => match *pat_type.pat.clone() {
                Pat::Ident(pat_ident) => {
                    let field_ident = &pat_ident.ident;
                    call_inputs.push(parse_quote!(#field_ident));
                }
                _ => panic!("all patterns should be verified to be idents"),
            },
        }
    }

    let handler_inputs: Vec<Ident> = action.inputs.iter().map(fn_arg_to_ident).collect();

    let handler_call = match &action.is_async {
        true => {
            parse_quote!(#shared_storage_path #shared_storage_turbofish ::#action_handler_name(#(#handler_inputs),*).await)
        }
        false => {
            parse_quote!(#shared_storage_path #shared_storage_turbofish ::#action_handler_name(#(#handler_inputs),*))
        }
    };

    Action { handler_call }
}

fn fn_arg_to_ident(fn_arg: &FnArg) -> Ident {
    match fn_arg {
        FnArg::Receiver(_) => parse_quote!(shared_storage),
        FnArg::Typed(pat_type) => match pat_type.pat.as_ref() {
            Pat::Ident(pat_ident) => pat_ident.ident.clone(),
            _ => panic!("all patterns should be verified to be idents"),
        },
    }
}

fn fn_arg_to_state_field(pat_type: &PatType) -> Field {
    let field_type = match pat_type.ty.as_ref() {
        Type::Reference(reference) => reference.elem.clone(),
        _ => abort!(pat_type, "input must be passed as a reference"),
    };

    match pat_type.pat.as_ref() {
        Pat::Ident(pat_ident) => {
            let field_ident = &pat_ident.ident;
            Field::parse_named
                .parse2(quote::quote!(#field_ident: #field_type))
                .unwrap()
        }
        _ => panic!("all patterns should be verified to be idents"),
    }
}

fn fn_arg_to_superstate_field(pat_type: &PatType) -> Field {
    let field_type = match pat_type.ty.as_ref() {
        Type::Reference(reference) => {
            let mut reference = reference.clone();
            reference.lifetime = Some(Lifetime::new(SUPERSTATE_LIFETIME, Span::call_site()));
            Type::Reference(reference)
        }
        _ => abort!(pat_type, "input must be passed as a reference"),
    };
    match pat_type.pat.as_ref() {
        Pat::Ident(pat_ident) => {
            let field_ident = &pat_ident.ident;
            Field::parse_named
                .parse2(quote::quote!(#field_ident: #field_type))
                .unwrap()
        }
        _ => panic!("all patterns should be verified to be idents"),
    }
}

pub fn get_generic_argument_ident(ty: &Type) -> Ident {
    match ty {
        Type::Path(path) => path.path.segments.last().map(|s| &s.ident).unwrap().clone(),
        _ => panic!("can not get ident of shared storage"),
    }
}

fn predicate_bounded_to_param(predicate: &WherePredicate, param: &GenericParam) -> bool {
    match (predicate, param) {
        (WherePredicate::Type(l), GenericParam::Type(r))
            if get_generic_argument_ident(&l.bounded_ty) == r.ident =>
        {
            true
        }
        (WherePredicate::Lifetime(l), GenericParam::Lifetime(r)) if l.lifetime == r.lifetime => {
            true
        }
        _ => false,
    }
}

/// Create hash map that associates certain generics with their predicates.
fn map_generics(generics: &Generics) -> Vec<(GenericParam, Vec<WherePredicate>)> {
    let mut map: Vec<_> = generics
        .params
        .iter()
        .map(|param| (param.clone(), Vec::new()))
        .collect();

    // Iterate over the generic parameters and add them to the map.
    for (param, predicates) in &mut map {
        for predicate in generics
            .where_clause
            .iter()
            .flat_map(|clause| &clause.predicates)
        {
            if predicate_bounded_to_param(predicate, param) {
                predicates.push(predicate.clone())
            }
        }
    }

    map
}

fn snake_case_to_pascal_case(snake: &Ident) -> Ident {
    let mut pascal = String::new();
    for part in snake.to_string().split('_') {
        let mut characters = part.chars();
        pascal.push_str(&characters.next().map_or_else(String::new, |c| {
            c.to_uppercase().chain(characters).collect()
        }));
    }
    format_ident!("{}", pascal)
}

#[cfg(test)]
fn create_analyze_state_machine() -> analyze::StateMachine {
    analyze::StateMachine {
        initial_state: parse_quote!(State::on()),
        shared_storage_type: parse_quote!(Blinky),
        shared_storage_path: parse_quote!(Blinky),
        shared_storage_generics: parse_quote!(),
        state_ident: parse_quote!(State),
        state_derives: vec![parse_quote!(Copy), parse_quote!(Clone)],
        superstate_ident: parse_quote!(Superstate),
        superstate_derives: vec![parse_quote!(Copy), parse_quote!(Clone)],
        on_transition: None,
        on_transition_async: None,
        on_dispatch: None,
        visibility: parse_quote!(pub),
        event_ident: parse_quote!(input),
        context_ident: parse_quote!(context),
    }
}

#[cfg(test)]
fn create_lower_state_machine() -> StateMachine {
    let mut superstate_generics = Generics::default();
    superstate_generics.params.push(parse_quote!('sub));
    StateMachine {
        initial_state: parse_quote!(State::on()),
        shared_storage_type: parse_quote!(Blinky),
        shared_storage_generics: parse_quote!(),
        event_type: parse_quote!(()),
        context_type: parse_quote!(()),
        #[rustfmt::skip]
        state_ident: parse_quote!(State),
        state_derives: vec![parse_quote!(Copy), parse_quote!(Clone)],
        state_generics: Generics::default(),
        superstate_ident: parse_quote!(Superstate),
        superstate_derives: vec![parse_quote!(Copy), parse_quote!(Clone)],
        superstate_generics,
        on_transition: None,
        on_transition_async: None,
        on_dispatch: None,
        visibility: parse_quote!(pub),
        event_ident: parse_quote!(input),
        context_ident: parse_quote!(context),
        mode: Mode::Blocking,
    }
}

#[cfg(test)]
fn create_analyze_state() -> analyze::State {
    analyze::State {
        handler_name: parse_quote!(on),
        superstate: parse_quote!(playing),
        entry_action: parse_quote!(enter_on),
        exit_action: None,
        local_storage: vec![],
        inputs: vec![
            parse_quote!(&mut self),
            parse_quote!(input: &Event),
            parse_quote!(led: &mut bool),
            parse_quote!(counter: &mut usize),
        ],
        shared_storage_input: Some(parse_quote!(&mut self)),
        event_arg: Some(
            if let FnArg::Typed(pat_type) = parse_quote!(event: &Event) {
                pat_type
            } else {
                panic!();
            },
        ),
        context_arg: None,
        state_inputs: vec![
            if let FnArg::Typed(pat_type) = parse_quote!(led: &mut bool) {
                pat_type
            } else {
                panic!();
            },
            if let FnArg::Typed(pat_type) = parse_quote!(counter: &mut usize) {
                pat_type
            } else {
                panic!();
            },
        ],
        is_async: false,
    }
}

#[cfg(test)]
fn create_lower_state() -> State {
    State {
        variant: parse_quote!(On {
            led: bool,
            counter: usize
        }),
        pat: parse_quote!(State::On { led, counter }),
        handler_call: parse_quote!(Blinky::on(shared_storage, input, led, counter)),
        entry_action_call: parse_quote!({}),
        exit_action_call: parse_quote!({}),
        superstate_pat: parse_quote!(None),
        constructor: parse_quote!(
            const fn on(led: bool, counter: usize) -> Self {
                Self::On { led, counter }
            }
        ),
    }
}

#[cfg(test)]
fn create_linked_lower_state() -> State {
    let mut state = create_lower_state();
    state.superstate_pat = parse_quote!(Some(Superstate::Playing { led, counter }));
    state.entry_action_call = parse_quote!(Blinky::enter_on(shared_storage, led));
    state
}

#[cfg(test)]
fn create_analyze_superstate() -> analyze::Superstate {
    analyze::Superstate {
        handler_name: parse_quote!(playing),
        superstate: None,
        entry_action: None,
        exit_action: None,
        local_storage: vec![],
        inputs: vec![
            parse_quote!(&mut self),
            parse_quote!(input: &Event),
            parse_quote!(led: &mut bool),
            parse_quote!(counter: &mut usize),
        ],
        shared_storage_input: Some(parse_quote!(&mut self)),
        event_arg: Some(
            if let FnArg::Typed(pat_type) = parse_quote!(event: &Event) {
                pat_type
            } else {
                panic!();
            },
        ),
        context_arg: None,
        state_inputs: vec![
            if let FnArg::Typed(pat_type) = parse_quote!(led: &mut bool) {
                pat_type
            } else {
                panic!();
            },
            if let FnArg::Typed(pat_type) = parse_quote!(counter: &mut usize) {
                pat_type
            } else {
                panic!();
            },
        ],
        is_async: false,
    }
}

#[cfg(test)]
fn create_lower_superstate() -> Superstate {
    Superstate {
        variant: parse_quote!(Playing {
            led: &'sub mut bool,
            counter: &'sub mut usize
        }),
        pat: parse_quote!(Superstate::Playing { led, counter }),
        handler_call: parse_quote!(Blinky::playing(shared_storage, input, led, counter)),
        entry_action_call: parse_quote!({}),
        exit_action_call: parse_quote!({}),
        superstate_pat: parse_quote!(None),
    }
}

#[cfg(test)]
fn create_analyze_action() -> analyze::Action {
    analyze::Action {
        handler_name: parse_quote!(enter_on),
        inputs: vec![parse_quote!(&mut self), parse_quote!(led: &mut bool)],
        is_async: false,
    }
}

#[cfg(test)]
fn create_lower_action() -> Action {
    Action {
        handler_call: parse_quote!(Blinky::enter_on(shared_storage, led)),
    }
}

#[cfg(test)]
fn create_analyze_model() -> analyze::Model {
    analyze::Model {
        item_impl: parse_quote!(impl Blinky { }),
        state_machine: create_analyze_state_machine(),
        states: [create_analyze_state()]
            .into_iter()
            .map(|state| (state.handler_name.clone(), state))
            .collect(),
        superstates: [create_analyze_superstate()]
            .into_iter()
            .map(|state| (state.handler_name.clone(), state))
            .collect(),
        actions: [create_analyze_action()]
            .into_iter()
            .map(|state| (state.handler_name.clone(), state))
            .collect(),
    }
}

#[cfg(test)]
fn create_lower_model() -> Ir {
    Ir {
        item_impl: parse_quote!(impl Blinky { }),
        state_machine: create_lower_state_machine(),
        states: [create_linked_lower_state()]
            .into_iter()
            .map(|state| (format_ident!("on"), state))
            .collect(),
        superstates: [create_lower_superstate()]
            .into_iter()
            .map(|state| (format_ident!("playing"), state))
            .collect(),
    }
}

#[test]
fn test_lower_state() {
    let analyze_state_machine = create_analyze_state_machine();
    let analyze_state = create_analyze_state();

    let actual = lower_state(&analyze_state, &analyze_state_machine);
    let expected = create_lower_state();

    assert_eq!(actual, expected);
}

#[test]
fn test_lower_superstate() {
    let analyze_state_machine = create_analyze_state_machine();
    let analyze_superstate = create_analyze_superstate();

    let actual = lower_superstate(&analyze_superstate, &analyze_state_machine);
    let expected = create_lower_superstate();

    assert_eq!(actual, expected);
}

#[test]
fn test_lower_action() {
    let state_machine = create_analyze_state_machine();
    let action = create_analyze_action();

    let actual = lower_action(&action, &state_machine);
    let expected = create_lower_action();

    assert_eq!(actual, expected);
}

#[test]
fn test_lower() {
    let model = create_analyze_model();

    let actual = lower(&model);
    let expected = create_lower_model();

    assert_eq!(actual, expected);
}
