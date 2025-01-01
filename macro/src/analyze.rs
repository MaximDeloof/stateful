use std::collections::HashMap;

use proc_macro_error::abort;
use syn::parse::Parser;
use syn::{
    parse_quote, Attribute, AttributeArgs, ExprCall, Field, FnArg, Generics, Ident, ImplItem,
    ImplItemMethod, ItemImpl, Lit, Meta, MetaList, NestedMeta, Pat, PatType, Path, Receiver, Type,
    Visibility,
};

/// Model of the state machine.
#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub struct Model {
    /// A copy of the item impl that was parsed.
    pub item_impl: ItemImpl,
    /// General information regarding the state machine.
    pub state_machine: StateMachine,
    /// The states of the state machine.
    pub states: HashMap<Ident, State>,
    /// The superstates of the state machine.
    pub superstates: HashMap<Ident, Superstate>,
    /// The actions of the state machine.
    pub actions: HashMap<Ident, Action>,
}

/// General information regarding the state machine.
#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub struct StateMachine {
    /// The inital state of the state machine.
    pub initial_state: ExprCall,
    /// The type on which the state machine is implemented.
    pub shared_storage_type: Type,
    /// The path of the shared storage.
    pub shared_storage_path: Path,
    /// The generics associated with the shared storage type.
    pub shared_storage_generics: Generics,
    /// The name for the state type.
    pub state_ident: Ident,
    /// Derives that will be applied on the state type.
    pub state_derives: Vec<Path>,
    /// The name of the superstate type.
    pub superstate_ident: Ident,
    /// Derives that will be applied to the superstate type.
    pub superstate_derives: Vec<Path>,
    /// The identifier that is used for the event argument.
    pub event_ident: Ident,
    /// The identifier that is used for the context argument.
    pub context_ident: Ident,
    /// The visibility of the derived types.
    pub visibility: Visibility,
    /// Optional `on_transition` callback.
    pub on_transition: Option<Path>,
    /// Optional `on_transition_async` callback.
    pub on_transition_async: Option<Path>,
    /// Optional `on_dispatch` callback.
    pub on_dispatch: Option<Path>,
}

/// Information regarding a state.
#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub struct State {
    /// Name of the state.
    pub handler_name: Ident,
    /// Optional superstate.
    pub superstate: Option<Ident>,
    /// Optional entry action.
    pub entry_action: Option<Ident>,
    /// Optional exit action.
    pub exit_action: Option<Ident>,
    /// Local storage,
    pub local_storage: Vec<Field>,
    /// Inputs required by the state handler.
    pub inputs: Vec<FnArg>,
    /// Optional receiver input for the state handler (e.g. `&mut self`).
    pub shared_storage_input: Option<Receiver>,
    /// Inputs provided by the state-local storage.
    pub state_inputs: Vec<PatType>,
    /// Event that is submitted to the state machine.
    pub event_arg: Option<PatType>,
    /// Context that is submitted to the state machine.
    pub context_arg: Option<PatType>,
    /// Whether the function is async or not.
    pub is_async: bool,
}

/// Information regarding a superstate.
#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub struct Superstate {
    /// Name of the superstate.
    pub handler_name: Ident,
    /// Optional superstate.
    pub superstate: Option<Ident>,
    /// Optional entry action.
    pub entry_action: Option<Ident>,
    /// Optional exit action.
    pub exit_action: Option<Ident>,
    /// Local storage,
    pub local_storage: Vec<Field>,
    /// Inputs required by the superstate handler.
    pub inputs: Vec<FnArg>,
    /// Optional receiver input for the state handler (e.g. `&mut self`).
    pub shared_storage_input: Option<Receiver>,
    /// Inputs provided by the state-local storage.
    pub state_inputs: Vec<PatType>,
    /// Event that is submitted to the state machine.
    pub event_arg: Option<PatType>,
    /// Context that is submitted to the state machine.
    pub context_arg: Option<PatType>,
    /// Whether the function is async or not.
    pub is_async: bool,
}

/// Information regarding an action.
#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub struct Action {
    /// Name of the action
    pub handler_name: Ident,
    /// Inputs required by the action handler.
    pub inputs: Vec<FnArg>,
    /// Whether the function is async or not.
    pub is_async: bool,
}

/// Analyze the impl block and create a model.
pub fn analyze(attribute_args: AttributeArgs, item_impl: ItemImpl) -> Model {
    let state_machine = analyze_state_machine(&attribute_args, &item_impl);

    let mut states = HashMap::new();
    let mut superstates = HashMap::new();
    let mut actions = HashMap::new();

    // Create an iterator over only the method items.
    let methods = item_impl.items.iter().filter_map(|item| match item {
        ImplItem::Method(method) => Some(method),
        _ => None,
    });

    // Iterator over the methods in the impl block.
    for method in methods {
        for attr in method.attrs.iter() {
            match &attr.path {
                path if path.is_ident("state") => {
                    let state = analyze_state(method, &state_machine);
                    states.insert(state.handler_name.clone(), state);
                }

                path if path.is_ident("superstate") => {
                    let superstate = analyze_superstate(method, &state_machine);
                    superstates.insert(superstate.handler_name.clone(), superstate);
                }

                path if path.is_ident("action") => {
                    let action = analyze_action(method);
                    actions.insert(action.handler_name.clone(), action);
                }

                _ => (),
            }
        }
    }

    Model {
        item_impl,
        state_machine,
        states,
        superstates,
        actions,
    }
}

/// Retrieve the top level settings of the state machine.
pub fn analyze_state_machine(attribute_args: &AttributeArgs, item_impl: &ItemImpl) -> StateMachine {
    let shared_storage_type = item_impl.self_ty.as_ref().clone();
    let shared_storage_generics = item_impl.generics.clone();
    let shared_storage_path = get_shared_storage_path(&shared_storage_type);

    let mut initial_state: Option<ExprCall> = None;

    let mut state_ident = parse_quote!(State);
    let mut state_derives = Vec::new();
    let mut superstate_ident = parse_quote!(Superstate);
    let mut superstate_derives = Vec::new();

    let mut on_transition = None;
    let mut on_transition_async = None;
    let mut on_dispatch = None;

    let mut visibility = parse_quote!(pub);
    let mut event_ident = parse_quote!(event);
    let mut context_ident = parse_quote!(context);

    let mut state_meta: MetaList = parse_quote!(state());
    let mut superstate_meta: MetaList = parse_quote!(superstate());

    // Iterate over the meta attribites on the `#[state_machine]` macro.
    for arg in attribute_args {
        match arg {
            NestedMeta::Meta(Meta::NameValue(name_value))
                if name_value.path.is_ident("initial") =>
            {
                initial_state = match &name_value.lit {
                    Lit::Str(input_pat) => input_pat.parse().ok(),
                    _ => abort!(name_value, "must be a string literal"),
                }
            }
            NestedMeta::Meta(Meta::NameValue(name_value))
                if name_value.path.is_ident("event_identifier") =>
            {
                event_ident = match &name_value.lit {
                    Lit::Str(event_ident) => event_ident.parse().unwrap(),
                    _ => abort!(name_value, "must be a string literal"),
                }
            }
            NestedMeta::Meta(Meta::NameValue(name_value))
                if name_value.path.is_ident("context_identifier") =>
            {
                context_ident = match &name_value.lit {
                    Lit::Str(context_ident) => context_ident.parse().unwrap(),
                    _ => abort!(name_value, "must be a string literal"),
                }
            }
            NestedMeta::Meta(Meta::NameValue(name_value))
                if name_value.path.is_ident("on_transition") =>
            {
                on_transition = match &name_value.lit {
                    Lit::Str(input_pat) => Some(input_pat.parse().unwrap()),
                    _ => abort!(name_value, "must be a string literal"),
                }
            }
            NestedMeta::Meta(Meta::NameValue(name_value))
                if name_value.path.is_ident("on_transition_async") =>
            {
                on_transition_async = match &name_value.lit {
                    Lit::Str(input_pat) => Some(input_pat.parse().unwrap()),
                    _ => abort!(name_value, "must be a string literal"),
                }
            }
            NestedMeta::Meta(Meta::NameValue(name_value))
                if name_value.path.is_ident("on_dispatch") =>
            {
                on_dispatch = match &name_value.lit {
                    Lit::Str(input_pat) => Some(input_pat.parse().unwrap()),
                    _ => abort!(name_value, "must be a string literal"),
                }
            }
            NestedMeta::Meta(Meta::NameValue(name_value))
                if name_value.path.is_ident("visibility") =>
            {
                visibility = match &name_value.lit {
                    Lit::Str(input_pat) => input_pat.parse().unwrap(),
                    _ => abort!(name_value, "must be a string literal"),
                }
            }
            NestedMeta::Meta(Meta::List(list)) if list.path.is_ident("state") => {
                state_meta = list.clone();
            }
            NestedMeta::Meta(Meta::List(list)) if list.path.is_ident("superstate") => {
                superstate_meta = list.clone();
            }

            _ => abort!(arg, "argument not recognized"),
        }
    }

    // Check if there is an initial state given.
    let Some(initial_state) = initial_state else {
        abort!(
            initial_state,
            "no initial state defined";
            help = "add an initial state `#[state_machine(initial = \"State::initial_state()\"]"
        );
    };

    // Iterate over the meta attributes for the state enum.
    for meta in state_meta
        .nested
        .iter()
        .filter_map(|nested_meta| match nested_meta {
            NestedMeta::Meta(meta) => Some(meta),
            NestedMeta::Lit(_) => None,
        })
    {
        match meta {
            // Get the custom name for the state enum.
            Meta::NameValue(name_value) if name_value.path.is_ident("name") => {
                state_ident = match &name_value.lit {
                    Lit::Str(str_lit) => str_lit.parse().unwrap(),
                    _ => abort!(name_value, "expected string literal"),
                }
            }

            // Get the derives for the state enum.
            Meta::List(meta_list) if meta_list.path.is_ident("derive") => {
                for nested_meta in &meta_list.nested {
                    match nested_meta {
                        NestedMeta::Meta(meta) => {
                            state_derives.push(meta.path().clone());
                        }
                        _ => abort!(nested_meta, "expected list of traits"),
                    }
                }
            }

            // Other attributes are not recognized.
            _ => abort!(meta, "unknown attribute"),
        }
    }

    // Iterate over the meta attributes for the superstate enum.
    for meta in superstate_meta
        .nested
        .iter()
        .filter_map(|nested_meta| match nested_meta {
            NestedMeta::Meta(meta) => Some(meta),
            NestedMeta::Lit(_) => None,
        })
    {
        match meta {
            // Get the custom name for the superstate enum.
            Meta::NameValue(name_value) if name_value.path.is_ident("name") => {
                superstate_ident = match &name_value.lit {
                    Lit::Str(str_lit) => str_lit.parse().unwrap(),
                    _ => abort!(name_value, "expected string literal"),
                }
            }

            // Get the derives of the superstate enum.
            Meta::List(meta_list) if meta_list.path.is_ident("derive") => {
                for nested_meta in &meta_list.nested {
                    match nested_meta {
                        NestedMeta::Meta(meta) => {
                            superstate_derives.push(meta.path().clone());
                        }
                        _ => abort!(nested_meta, "expected list of traits"),
                    }
                }
            }

            // Other attributes are not recognized.
            _ => abort!(meta, "unknown attribute"),
        }
    }

    StateMachine {
        initial_state,
        shared_storage_type,
        shared_storage_path,
        shared_storage_generics,
        state_ident,
        state_derives,
        superstate_ident,
        superstate_derives,
        on_dispatch,
        on_transition,
        on_transition_async,
        event_ident,
        context_ident,
        visibility,
    }
}

/// Retrieve information regarding the state.
pub fn analyze_state(method: &ImplItemMethod, state_machine: &StateMachine) -> State {
    let handler_name = method.sig.ident.clone();
    let inputs = method.sig.inputs.iter().cloned().collect();

    let mut superstate = None;
    let mut entry_action = None;
    let mut exit_action = None;
    let mut local_storage = Vec::new();
    let mut shared_storage_input = None;
    let mut state_inputs = Vec::new();
    let mut event_arg = None;
    let mut context_arg = None;

    let generic_params = &method.sig.generics.params;
    if !generic_params.is_empty() {
        abort!(
            generic_params,
            "state handlers can not define their generics themselves";
            help = "consider declaring the generics on the impl block"
        )
    }

    let is_async = method.sig.asyncness.is_some();

    // Iterate over the inputs of the state handler.
    for input in &method.sig.inputs {
        match input {
            FnArg::Receiver(receiver) => shared_storage_input = Some(receiver.clone()),
            FnArg::Typed(pat_type) => match *pat_type.pat.clone() {
                Pat::Ident(pat) if state_machine.event_ident.eq(&pat.ident) => {
                    event_arg = Some(pat_type.clone());
                }
                Pat::Ident(pat) if state_machine.context_ident.eq(&pat.ident) => {
                    context_arg = Some(pat_type.clone());
                }
                Pat::Ident(_) => {
                    state_inputs.push(pat_type.clone());
                }
                Pat::Reference(_) => {
                    state_inputs.push(pat_type.clone());
                }
                Pat::Tuple(_) => abort!(pat_type, "tuple pattern is not supported"),
                Pat::TupleStruct(_) => abort!(pat_type, "tuple struct pattern is not supported"),
                Pat::Struct(_) => abort!(pat_type, "struct pattern is not supported"),
                Pat::Wild(_) => abort!(
                    pat_type,
                    "wildcard pattern is not supported";
                    help = "consider giving the input a name"
                ),
                _ => abort!(pat_type, "patterns are not supported"),
            },
        }
    }

    // Iterate over the meta attributes on the state handler.
    for meta in get_meta(&method.attrs, "state") {
        match meta {
            Meta::NameValue(name_value) if name_value.path.is_ident("superstate") => {
                if let Lit::Str(value) = name_value.lit {
                    superstate = Some(Ident::new(&value.value(), value.span()));
                }
            }
            Meta::NameValue(name_value) if name_value.path.is_ident("entry_action") => {
                if let Lit::Str(value) = name_value.lit {
                    entry_action = Some(Ident::new(&value.value(), value.span()));
                }
            }
            Meta::NameValue(name_value) if name_value.path.is_ident("exit_action") => {
                if let Lit::Str(value) = name_value.lit {
                    exit_action = Some(Ident::new(&value.value(), value.span()));
                }
            }
            Meta::List(list) if list.path.is_ident("local_storage") => {
                for item in list.nested {
                    if let NestedMeta::Lit(Lit::Str(value)) = item {
                        let field = value.value();
                        local_storage.push(Field::parse_named.parse_str(&field).unwrap());
                    }
                }
            }
            _ => abort!(meta, "unknown attribute"),
        }
    }

    State {
        handler_name,
        superstate,
        entry_action,
        exit_action,
        local_storage,
        inputs,
        shared_storage_input,
        state_inputs,
        event_arg,
        context_arg,
        is_async,
    }
}

/// Retrieve the information regarding the superstate.
pub fn analyze_superstate(method: &ImplItemMethod, state_machine: &StateMachine) -> Superstate {
    let handler_name = method.sig.ident.clone();
    let inputs = method.sig.inputs.iter().cloned().collect();

    let mut superstate = None;
    let mut entry_action = None;
    let mut exit_action = None;
    let mut local_storage = Vec::new();
    let mut shared_storage_input = None;
    let mut state_inputs = Vec::new();
    let mut event_arg = None;
    let mut context_arg = None;

    let generic_params = &method.sig.generics.params;
    if !generic_params.is_empty() {
        abort!(
            generic_params,
            "superstate handlers can not define their generics themselves";
            help = "consider declaring the generics on the impl block"
        )
    }

    let is_async = method.sig.asyncness.is_some();

    // Iterate over the inputs of the superstate handler.
    for input in &method.sig.inputs {
        match input {
            FnArg::Receiver(receiver) => shared_storage_input = Some(receiver.clone()),
            FnArg::Typed(pat_type) => match *pat_type.pat.clone() {
                Pat::Ident(pat) if state_machine.event_ident.eq(&pat.ident) => {
                    event_arg = Some(pat_type.clone());
                }
                Pat::Ident(pat) if state_machine.context_ident.eq(&pat.ident) => {
                    context_arg = Some(pat_type.clone());
                }
                Pat::Ident(_) => {
                    state_inputs.push(pat_type.clone());
                }
                Pat::Reference(_) => {
                    state_inputs.push(pat_type.clone());
                }
                Pat::Tuple(_) => abort!(pat_type, "tuple pattern is not supported"),
                Pat::TupleStruct(_) => abort!(pat_type, "tuple struct pattern is not supported"),
                Pat::Struct(_) => abort!(pat_type, "struct pattern is not supported"),
                Pat::Wild(_) => abort!(
                    pat_type,
                    "wildcard pattern is not supported";
                    help = "consider giving the input a name"
                ),
                _ => abort!(pat_type, "patterns are not supported"),
            },
        }
    }

    // Iterate over the meta attributes on the superstate handler.
    for meta in get_meta(&method.attrs, "superstate") {
        match meta {
            Meta::NameValue(name_value) if name_value.path.is_ident("superstate") => {
                if let Lit::Str(value) = name_value.lit {
                    superstate = Some(Ident::new(&value.value(), value.span()));
                }
            }
            Meta::NameValue(name_value) if name_value.path.is_ident("entry_action") => {
                if let Lit::Str(value) = name_value.lit {
                    entry_action = Some(Ident::new(&value.value(), value.span()));
                }
            }
            Meta::NameValue(name_value) if name_value.path.is_ident("exit_action") => {
                if let Lit::Str(value) = name_value.lit {
                    exit_action = Some(Ident::new(&value.value(), value.span()));
                }
            }
            Meta::List(list) if list.path.is_ident("local_storage") => {
                for item in list.nested {
                    if let NestedMeta::Lit(Lit::Str(value)) = item {
                        let field = value.value();
                        local_storage.push(Field::parse_named.parse_str(&field).unwrap());
                    }
                }
            }
            _ => abort!(meta, "unknown attribute"),
        }
    }

    Superstate {
        handler_name,
        superstate,
        entry_action,
        exit_action,
        local_storage,
        inputs,
        shared_storage_input,
        state_inputs,
        event_arg,
        context_arg,
        is_async,
    }
}

/// Retrieve the information regarding the action.
pub fn analyze_action(method: &ImplItemMethod) -> Action {
    let handler_name = method.sig.ident.clone();
    let inputs = method.sig.inputs.clone().into_iter().collect();
    let is_async = method.sig.asyncness.is_some();

    let generic_params = &method.sig.generics.params;
    if !generic_params.is_empty() {
        abort!(
            generic_params,
            "action handlers can not define their generics themselves";
            help = "consider declaring the generics on the impl block"
        )
    }

    Action {
        handler_name,
        inputs,
        is_async,
    }
}

/// Parse the attributes as a meta item.
pub fn get_meta(attrs: &[Attribute], name: &str) -> Vec<Meta> {
    attrs
        .iter()
        .filter(|attr| attr.path.is_ident(name))
        .filter_map(|attr| attr.parse_meta().ok())
        .filter_map(|meta| match meta {
            Meta::List(list_meta) => Some(list_meta.nested),
            _ => None,
        })
        .flatten()
        .filter_map(|nested| match nested {
            NestedMeta::Meta(meta) => Some(meta),
            _ => None,
        })
        .collect()
}

/// Get the ident of the shared storage type.
pub fn get_shared_storage_path(ty: &Type) -> Path {
    match ty {
        Type::Path(path) => {
            let segments: Vec<_> = path.path.segments.iter().map(|seg| &seg.ident).collect();
            parse_quote!(#(#segments)::*)
        }
        _ => panic!("can not get path of shared storage"),
    }
}

#[test]
fn valid_state_analyze() {
    use syn::parse_quote;

    let init_arg: NestedMeta = parse_quote!(initial = "State::on()");
    let input_arg: NestedMeta = parse_quote!(event_identifier = "event");
    let state_arg: NestedMeta = parse_quote!(state(derive(Copy, Clone)));
    let superstate_arg: NestedMeta = parse_quote!(superstate(derive(Copy, Clone)));
    let attribute_args = vec![init_arg, input_arg, state_arg, superstate_arg];

    let item_impl: ItemImpl = parse_quote!(
        impl Blinky {
            #[state(
                superstate = "playing",
                entry_action = "enter_on",
                exit_action = "enter_off"
            )]
            fn on(&mut self, event: &Event) -> Response<State> {
                Response::Handled
            }

            #[superstate]
            fn playing(&mut self, event: &Event) -> Response<State> {
                Response::Handled
            }

            #[action]
            fn enter_on(&mut self) {}

            #[action]
            fn enter_off(&mut self) {}
        }
    );

    let actual = analyze(attribute_args, item_impl.clone());

    let initial_state = parse_quote!(State::on());

    let shared_storage_type = parse_quote!(Blinky);
    let shared_storage_path = parse_quote!(Blinky);
    let shared_storage_generics = parse_quote!();

    let state_ident = parse_quote!(State);
    let state_derives = vec![parse_quote!(Copy), parse_quote!(Clone)];
    let superstate_ident = parse_quote!(Superstate);
    let superstate_derives = vec![parse_quote!(Copy), parse_quote!(Clone)];
    let on_transition = None;
    let on_dispatch = None;
    let event_ident = parse_quote!(event);
    let context_ident = parse_quote!(context);
    let visibility = parse_quote!(pub);

    let state_machine = StateMachine {
        initial_state,
        shared_storage_type,
        shared_storage_path,
        shared_storage_generics,
        state_ident,
        state_derives,
        superstate_ident,
        superstate_derives,
        on_transition,
        on_transition_async: None,
        on_dispatch,
        event_ident,
        context_ident,
        visibility,
    };

    let state = State {
        handler_name: parse_quote!(on),
        superstate: parse_quote!(playing),
        entry_action: parse_quote!(enter_on),
        exit_action: parse_quote!(enter_off),
        local_storage: vec![],
        inputs: vec![parse_quote!(&mut self), parse_quote!(event: &Event)],
        shared_storage_input: Some(parse_quote!(&mut self)),
        state_inputs: vec![],
        event_arg: Some(if let FnArg::Typed(event) = parse_quote!(event: &Event) {
            event
        } else {
            return;
        }),
        context_arg: None,
        is_async: false,
    };

    let superstate = Superstate {
        handler_name: parse_quote!(playing),
        superstate: None,
        entry_action: None,
        exit_action: None,
        local_storage: vec![],
        inputs: vec![parse_quote!(&mut self), parse_quote!(event: &Event)],
        shared_storage_input: Some(parse_quote!(&mut self)),
        state_inputs: vec![],
        event_arg: Some(if let FnArg::Typed(event) = parse_quote!(event: &Event) {
            event
        } else {
            return;
        }),
        context_arg: None,
        is_async: false,
    };

    let entry_action = Action {
        handler_name: parse_quote!(enter_on),
        inputs: vec![parse_quote!(&mut self)],
        is_async: false,
    };

    let exit_action = Action {
        handler_name: parse_quote!(enter_off),
        inputs: vec![parse_quote!(&mut self)],
        is_async: false,
    };

    let mut states = HashMap::new();
    let mut superstates = HashMap::new();
    let mut actions = HashMap::new();

    states.insert(state.handler_name.clone(), state);
    superstates.insert(superstate.handler_name.clone(), superstate);
    actions.insert(entry_action.handler_name.clone(), entry_action);
    actions.insert(exit_action.handler_name.clone(), exit_action);

    let expected = Model {
        item_impl,
        state_machine,
        states,
        superstates,
        actions,
    };

    assert_eq!(actual, expected);
}
