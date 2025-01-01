use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{
    parse_quote, Arm, GenericParam, ItemEnum, ItemFn, ItemImpl, Lifetime, LifetimeDef, Variant,
};

use crate::lower::{Ir, Mode};
use crate::{CONTEXT_LIFETIME, EVENT_LIFETIME, SUPERSTATE_LIFETIME};

pub fn codegen(ir: Ir) -> TokenStream {
    let item_impl = &ir.item_impl;

    let state_machine_impl = codegen_state_machine_impl(&ir);

    let state_enum = codegen_state(&ir);
    let state_impl = codegen_state_impl(&ir);
    let state_impl_state = codegen_state_impl_state(&ir);
    let superstate_enum = codegen_superstate(&ir);
    let superstate_impl = codegen_superstate_impl_superstate(&ir);

    quote!(
        // Import the proc_macro attributes so they can be used to tag functions.
        use statig::{state, superstate, action};

        #item_impl

        #state_machine_impl

        #state_enum

        #state_impl

        #state_impl_state

        #superstate_enum

        #superstate_impl
    )
}
fn codegen_state_machine_impl(ir: &Ir) -> ItemImpl {
    let shared_storage_type = &ir.state_machine.shared_storage_type;
    let (impl_generics, _, where_clause) =
        &ir.state_machine.shared_storage_generics.split_for_impl();
    let event_type = &ir.state_machine.event_type;
    let context_type = &ir.state_machine.context_type;
    let state_ident = &ir.state_machine.state_ident;
    let (_, state_generics, _) = &ir.state_machine.state_generics.split_for_impl();
    let superstate_ident = &ir.state_machine.superstate_ident;
    let (_, superstate_generics, _) = &ir.state_machine.superstate_generics.split_for_impl();
    let superstate_lifetime = Lifetime::new(SUPERSTATE_LIFETIME, Span::call_site());
    let event_lifetime = Lifetime::new(EVENT_LIFETIME, Span::call_site());
    let context_lifetime = Lifetime::new(CONTEXT_LIFETIME, Span::call_site());

    let initial_state = &ir.state_machine.initial_state;

    let mode = match ir.state_machine.mode {
        Mode::Blocking => quote!(blocking),
        Mode::Awaitable => quote!(awaitable),
    };

    let on_transition = match &ir.state_machine.on_transition {
        None => quote!(),
        Some(on_transition) => quote!(
            const ON_TRANSITION: fn(&mut Self, &Self::State, &Self::State) = #on_transition;
        ),
    };

    let on_transition_async = match &ir.state_machine.on_transition_async {
        None => quote!(),
        Some(on_transition_async) => quote!(
            const ON_TRANSITION_ASYNC: for <'a> fn(&'a mut Self, &'a Self::State, &'a Self::State) -> core::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> = #on_transition_async;
        ),
    };

    let on_dispatch = match &ir.state_machine.on_dispatch {
        None => quote!(),
        Some(on_dispatch) => quote!(
            const ON_DISPATCH: fn(&mut Self, StateOrSuperstate<'_, '_, Self>, &Self::Event<'_>) = #on_dispatch;
        ),
    };

    parse_quote!(
        impl #impl_generics statig::#mode::IntoStateMachine for #shared_storage_type #where_clause
        {
            type Event<#event_lifetime> = #event_type;
            type Context<#context_lifetime> = #context_type;
            type State = #state_ident #state_generics;
            type Superstate<#superstate_lifetime> = #superstate_ident #superstate_generics ;
            const INITIAL: #state_ident #state_generics = #initial_state;

            #on_transition

            #on_transition_async

            #on_dispatch
        }
    )
}

fn codegen_state(ir: &Ir) -> ItemEnum {
    let state_ident = &ir.state_machine.state_ident;
    let (state_generics, _, _) = &ir.state_machine.state_generics.split_for_impl();
    let state_derives = &ir.state_machine.state_derives;

    let variants: Vec<Variant> = ir
        .states
        .values()
        .map(|state| state.variant.clone())
        .collect();
    let visibility = &ir.state_machine.visibility;

    parse_quote!(
        #[derive(#(#state_derives),*)]
        # visibility enum #state_ident #state_generics {
            #(#variants),*
        }
    )
}

fn codegen_state_impl(ir: &Ir) -> ItemImpl {
    let state_ident = &ir.state_machine.state_ident;
    let (impl_generics, state_generics, _) = &ir.state_machine.state_generics.split_for_impl();

    let constructors: Vec<ItemFn> = ir
        .states
        .values()
        .map(|state| &state.constructor)
        .cloned()
        .collect();

    parse_quote!(
        impl #impl_generics #state_ident #state_generics {
            #(#constructors)*
        }
    )
}

fn codegen_state_impl_state(ir: &Ir) -> ItemImpl {
    let shared_storage_type = &ir.state_machine.shared_storage_type;
    let (impl_generics, _, where_clause) =
        &ir.state_machine.shared_storage_generics.split_for_impl();
    let state_ident = &ir.state_machine.state_ident;
    let (_, state_generics, _) = &ir.state_machine.state_generics.split_for_impl();
    let event_ident = &ir.state_machine.event_ident;
    let context_ident = &ir.state_machine.context_ident;

    let mut constructors: Vec<ItemFn> = Vec::new();
    let mut call_handler_arms: Vec<Arm> = Vec::new();
    let mut call_entry_action_arms: Vec<Arm> = Vec::new();
    let mut call_exit_action_arms: Vec<Arm> = Vec::new();
    let mut superstate_arms: Vec<Arm> = Vec::new();
    let mut same_state_arms: Vec<Arm> = Vec::new();

    for state in ir.states.values() {
        let pat = &state.pat;
        let handler_call = &state.handler_call;
        let entry_action_call = &state.entry_action_call;
        let exit_action_call = &state.exit_action_call;
        let superstate_pat = &state.superstate_pat;

        constructors.push(state.constructor.clone());
        call_handler_arms.push(parse_quote!(#pat => #handler_call));
        call_entry_action_arms.push(parse_quote!(#pat => #entry_action_call));
        call_exit_action_arms.push(parse_quote!(#pat => #exit_action_call));
        superstate_arms.push(parse_quote!(#pat => #superstate_pat));
    }

    call_handler_arms.push(parse_quote!(_ => statig::Response::Super));
    call_entry_action_arms.push(parse_quote!(_ => {}));
    call_exit_action_arms.push(parse_quote!(_ => {}));
    superstate_arms.push(parse_quote!(_ => None));
    same_state_arms.push(parse_quote!(_ => false));

    match ir.state_machine.mode {
        Mode::Blocking => {
            parse_quote!(
                #[allow(unused)]
                impl #impl_generics statig::blocking::State<#shared_storage_type> for #state_ident #state_generics #where_clause
                {
                    fn call_handler(
                        &mut self,
                        shared_storage: &mut #shared_storage_type,
                        #event_ident: &<#shared_storage_type as statig::IntoStateMachine>::Event<'_>,
                        #context_ident: &mut <#shared_storage_type as statig::IntoStateMachine>::Context<'_>
                    ) -> statig::Response<Self> where Self: Sized {
                        match self {
                            #(#call_handler_arms),*
                        }
                    }

                    fn call_entry_action(
                        &mut self,
                        shared_storage: &mut #shared_storage_type,
                        #context_ident: &mut <#shared_storage_type as statig::IntoStateMachine>::Context<'_>
                    ) {
                        match self {
                            #(#call_entry_action_arms),*
                        }
                    }

                    fn call_exit_action(
                        &mut self,
                        shared_storage: &mut #shared_storage_type,
                        #context_ident: &mut <#shared_storage_type as statig::IntoStateMachine>::Context<'_>
                    ) {
                        match self {
                            #(#call_exit_action_arms),*
                        }
                    }

                    fn superstate(&mut self) -> Option<<#shared_storage_type as statig::IntoStateMachine>::Superstate<'_>> {
                        match self {
                            #(#superstate_arms),*
                        }
                    }
                }
            )
        }
        Mode::Awaitable => parse_quote!(
            #[allow(unused)]
            impl #impl_generics statig::awaitable::State<#shared_storage_type> for #state_ident #state_generics #where_clause
            {
                fn call_handler<'fut>(
                    &'fut mut self,
                    shared_storage: &'fut mut #shared_storage_type,
                    #event_ident: &'fut <#shared_storage_type as statig::IntoStateMachine>::Event<'_>,
                    #context_ident: &'fut mut <#shared_storage_type as statig::IntoStateMachine>::Context<'_>
                ) -> core::pin::Pin<std::boxed::Box<dyn core::future::Future<Output = statig::Response<Self>> + 'fut + Send>> {
                    Box::pin(async move {
                        match self {
                            #(#call_handler_arms),*
                        }
                    })
                }

                fn call_entry_action<'fut>(
                    &'fut mut self,
                    shared_storage: &'fut mut #shared_storage_type,
                    #context_ident: &'fut mut <#shared_storage_type as statig::IntoStateMachine>::Context<'_>
                ) -> core::pin::Pin<std::boxed::Box<dyn core::future::Future<Output = ()> + 'fut + Send>> {
                    Box::pin(async move {
                        match self {
                            #(#call_entry_action_arms),*
                        }
                    })
                }

                fn call_exit_action<'fut>(
                    &'fut mut self,
                    shared_storage: &'fut mut #shared_storage_type,
                    #context_ident: &'fut mut <#shared_storage_type as statig::IntoStateMachine>::Context<'_>
                ) -> core::pin::Pin<std::boxed::Box<dyn core::future::Future<Output = ()> + 'fut + Send>> {
                    Box::pin(async move {
                        match self {
                            #(#call_exit_action_arms),*
                        }
                    })
                }

                fn superstate(&mut self) -> Option<<#shared_storage_type as statig::IntoStateMachine>::Superstate<'_>> {
                    match self {
                        #(#superstate_arms),*
                    }
                }
            }
        ),
    }
}

fn codegen_superstate(ir: &Ir) -> ItemEnum {
    let superstate_ident = &ir.state_machine.superstate_ident;
    let (superstate_generics, _, _) = &ir.state_machine.superstate_generics.split_for_impl();
    let superstate_derives = &ir.state_machine.superstate_derives;

    let variants: Vec<Variant> = ir
        .superstates
        .values()
        .map(|superstate| superstate.variant.clone())
        .collect();
    let visibility = &ir.state_machine.visibility;

    parse_quote!(
        #[derive(#(#superstate_derives),*)]
        #visibility enum #superstate_ident #superstate_generics {
            #(#variants),*
        }
    )
}

fn codegen_superstate_impl_superstate(ir: &Ir) -> ItemImpl {
    let shared_storage_type = &ir.state_machine.shared_storage_type;
    let mut shared_storage_generics = ir.state_machine.shared_storage_generics.clone();
    let lifetime = Lifetime::new(SUPERSTATE_LIFETIME, Span::call_site());
    let superstate_lifetime_def = LifetimeDef::new(lifetime.clone());
    let superstate_lifetime_param = GenericParam::Lifetime(superstate_lifetime_def);
    shared_storage_generics
        .params
        .push(superstate_lifetime_param);
    match &mut shared_storage_generics.where_clause {
        Some(clause) => clause.predicates.push(parse_quote!(Self: #lifetime)),
        None => shared_storage_generics.where_clause = parse_quote!(where Self: #lifetime),
    }
    let (impl_generics, _, where_clause) = shared_storage_generics.split_for_impl();
    let superstate_ident = &ir.state_machine.superstate_ident;
    let (_, superstate_generics, _) = &ir.state_machine.superstate_generics.split_for_impl();
    let event_ident = &ir.state_machine.event_ident;
    let context_ident = &ir.state_machine.context_ident;

    let mut call_handler_arms: Vec<Arm> = Vec::new();
    let mut call_entry_action_arms: Vec<Arm> = Vec::new();
    let mut call_exit_action_arms: Vec<Arm> = Vec::new();
    let mut superstate_arms: Vec<Arm> = Vec::new();
    let mut same_state_arms: Vec<Arm> = Vec::new();

    for state in ir.superstates.values() {
        let pat = &state.pat;
        let handler_call = &state.handler_call;
        let entry_action_call = &state.entry_action_call;
        let exit_action_call = &state.exit_action_call;
        let superstate_pat = &state.superstate_pat;

        call_handler_arms.push(parse_quote!(#pat => #handler_call));
        call_entry_action_arms.push(parse_quote!(#pat => #entry_action_call));
        call_exit_action_arms.push(parse_quote!(#pat => #exit_action_call));
        superstate_arms.push(parse_quote!(#pat => #superstate_pat));
    }

    call_handler_arms.push(parse_quote!(_ => statig::Response::Super));
    call_entry_action_arms.push(parse_quote!(_ => {}));
    call_exit_action_arms.push(parse_quote!(_ => {}));
    superstate_arms.push(parse_quote!(_ => None));
    same_state_arms.push(parse_quote!(_ => false));

    match ir.state_machine.mode {
        Mode::Blocking => {
            parse_quote!(
                #[allow(unused)]
                impl #impl_generics statig::blocking::Superstate<#shared_storage_type> for #superstate_ident #superstate_generics #where_clause
                {
                    fn call_handler(
                        &mut self,
                        shared_storage: &mut #shared_storage_type,
                        #event_ident: &<#shared_storage_type as statig::IntoStateMachine>::Event<'_>,
                        #context_ident: &mut <#shared_storage_type as statig::IntoStateMachine>::Context<'_>
                    ) -> statig::Response<<#shared_storage_type as statig::IntoStateMachine>::State> where Self: Sized {
                        match self {
                            #(#call_handler_arms),*
                        }
                    }

                    fn call_entry_action(
                        &mut self,
                        shared_storage: &mut #shared_storage_type,
                        #context_ident: &mut <#shared_storage_type as statig::IntoStateMachine>::Context<'_>
                    ) {
                        match self {
                            #(#call_entry_action_arms),*
                        }
                    }

                    fn call_exit_action(
                        &mut self,
                        shared_storage: &mut #shared_storage_type,
                        #context_ident: &mut <#shared_storage_type as statig::IntoStateMachine>::Context<'_>
                    ) {
                        match self {
                            #(#call_exit_action_arms),*
                        }
                    }

                    fn superstate(&mut self) -> Option<<#shared_storage_type as statig::IntoStateMachine>::Superstate<'_>> {
                        match self {
                            #(#superstate_arms),*
                        }
                    }
                }
            )
        }
        Mode::Awaitable => {
            parse_quote!(
                #[allow(unused)]
                impl #impl_generics statig::awaitable::Superstate<#shared_storage_type> for #superstate_ident #superstate_generics #where_clause
                {
                    fn call_handler<'fut>(
                        &'fut mut self,
                        shared_storage: &'fut mut #shared_storage_type,
                        #event_ident: &'fut <#shared_storage_type as statig::IntoStateMachine>::Event<'_>,
                        #context_ident: &'fut mut <#shared_storage_type as statig::IntoStateMachine>::Context<'_>
                    ) -> core::pin::Pin<std::boxed::Box<dyn core::future::Future<Output = statig::Response<<#shared_storage_type as statig::IntoStateMachine>::State>> + 'fut + Send>> {
                        Box::pin(async move {
                            match self {
                                #(#call_handler_arms),*
                            }
                        })
                    }

                    fn call_entry_action<'fut>(
                        &'fut mut self,
                        shared_storage: &'fut mut #shared_storage_type,
                        #context_ident: &'fut mut <#shared_storage_type as statig::IntoStateMachine>::Context<'_>
                    ) -> core::pin::Pin<std::boxed::Box<dyn core::future::Future<Output = ()> + 'fut + Send>> {
                        Box::pin(async move {
                            match self {
                                #(#call_entry_action_arms),*
                            }
                        })
                    }

                    fn call_exit_action<'fut>(
                        &'fut mut self,
                        shared_storage: &'fut mut #shared_storage_type,
                        #context_ident: &'fut mut <#shared_storage_type as statig::IntoStateMachine>::Context<'_>
                    ) -> core::pin::Pin<std::boxed::Box<dyn core::future::Future<Output = ()> + 'fut + Send>> {
                        Box::pin(async move {
                            match self {
                                #(#call_exit_action_arms),*
                            }
                        })
                    }

                    fn superstate(&mut self) -> Option<<#shared_storage_type as statig::IntoStateMachine>::Superstate<'_>> {
                        match self {
                            #(#superstate_arms),*
                        }
                    }
                }
            )
        }
    }
}
