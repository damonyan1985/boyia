//! `#[boyia_class]` — compile-time class registrar with unrolled `attach_method` calls.
//! Child fns use `#[boyia_async_builtin]` or `#[boyia_sync_builtin]` (parsed by this macro).
//! `#[boyia_native_object]` stores instance state in `nativePtr` + `Box`.

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{format_ident, quote};
use syn::{
    parse::Parse, parse_macro_input, token::Comma, Attribute, Expr, ExprLit, Field, Fields, FnArg,
    ImplItem, ImplItemFn, ItemFn, ItemImpl, ItemStruct, Lit, LitStr, Meta, Pat, PatType, Type,
    TypePath,
};

struct ClassConfig {
    name: LitStr,
    registrar: syn::Ident,
    native: bool,
}

impl Parse for ClassConfig {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut name = None;
        let mut registrar = None;
        let mut native = false;

        while !input.is_empty() {
            let key: syn::Ident = input.parse()?;
            if key == "native" {
                native = true;
                if input.peek(Comma) {
                    input.parse::<Comma>()?;
                }
                continue;
            }
            input.parse::<syn::Token![=]>()?;
            match key.to_string().as_str() {
                "name" => name = Some(parse_string_lit(input)?),
                "registrar" => registrar = Some(input.parse()?),
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!(
                            "unknown key `{other}`, expected `name`, `registrar`, or `native`"
                        ),
                    ));
                }
            }
            if input.peek(Comma) {
                input.parse::<Comma>()?;
            }
        }

        Ok(ClassConfig {
            name: name.ok_or_else(|| {
                syn::Error::new(Span::call_site(), "missing `name = \"...\"` in attribute")
            })?,
            registrar: registrar.ok_or_else(|| {
                syn::Error::new(Span::call_site(), "missing `registrar = ...` in attribute")
            })?,
            native,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SelfArg {
    None,
    Ref,
    Mut,
}

struct AsyncMethodConfig {
    native: Option<syn::Ident>,
    method: LitStr,
    before: Option<syn::Path>,
}

struct SyncMethodConfig {
    native: Option<syn::Ident>,
    method: LitStr,
}

enum BuiltinKind {
    Async(AsyncMethodConfig),
    Sync(SyncMethodConfig),
}

fn parse_string_lit(input: syn::parse::ParseStream) -> syn::Result<LitStr> {
    let expr: Expr = input.parse()?;
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Str(lit), ..
        }) => Ok(lit),
        other => Err(syn::Error::new_spanned(
            other,
            "expected a string literal",
        )),
    }
}

fn expr_to_path(expr: Expr) -> syn::Result<syn::Path> {
    match expr {
        Expr::Path(p) => Ok(p.path),
        other => Err(syn::Error::new_spanned(
            other,
            "expected a function path for `before`",
        )),
    }
}

fn parse_async_config(meta: &Meta) -> syn::Result<AsyncMethodConfig> {
    let Meta::List(list) = meta else {
        return Err(syn::Error::new_spanned(
            meta,
            "expected `#[boyia_async_builtin(method = \"...\")]`",
        ));
    };
    syn::parse2(list.tokens.clone())
}

impl Parse for AsyncMethodConfig {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut native = None;
        let mut method = None;
        let mut before = None;

        while !input.is_empty() {
            let key: syn::Ident = input.parse()?;
            input.parse::<syn::Token![=]>()?;
            match key.to_string().as_str() {
                "native" => native = Some(input.parse()?),
                "method" => method = Some(parse_string_lit(input)?),
                "before" => {
                    let expr: Expr = input.parse()?;
                    before = Some(expr_to_path(expr)?);
                }
                "class" => {
                    return Err(syn::Error::new(
                        key.span(),
                        "`class` belongs on `#[boyia_class]`, not on `#[boyia_async_builtin]`",
                    ));
                }
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!(
                            "unknown key `{other}`, expected `method`, `native`, or `before`"
                        ),
                    ));
                }
            }
            if input.peek(Comma) {
                input.parse::<Comma>()?;
            }
        }

        Ok(AsyncMethodConfig {
            native,
            method: method.ok_or_else(|| {
                syn::Error::new(Span::call_site(), "missing `method = \"...\"` in attribute")
            })?,
            before,
        })
    }
}

fn parse_sync_config(meta: &Meta) -> syn::Result<SyncMethodConfig> {
    let Meta::List(list) = meta else {
        return Err(syn::Error::new_spanned(
            meta,
            "expected `#[boyia_sync_builtin(method = \"...\")]`",
        ));
    };
    syn::parse2(list.tokens.clone())
}

impl Parse for SyncMethodConfig {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut native = None;
        let mut method = None;

        while !input.is_empty() {
            let key: syn::Ident = input.parse()?;
            input.parse::<syn::Token![=]>()?;
            match key.to_string().as_str() {
                "native" => native = Some(input.parse()?),
                "method" => method = Some(parse_string_lit(input)?),
                "before" => {
                    return Err(syn::Error::new(
                        key.span(),
                        "`before` is only supported on `#[boyia_async_builtin]`",
                    ));
                }
                "class" => {
                    return Err(syn::Error::new(
                        key.span(),
                        "`class` belongs on `#[boyia_class]`, not on `#[boyia_sync_builtin]`",
                    ));
                }
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("unknown key `{other}`, expected `method` or `native`"),
                    ));
                }
            }
            if input.peek(Comma) {
                input.parse::<Comma>()?;
            }
        }

        Ok(SyncMethodConfig {
            native,
            method: method.ok_or_else(|| {
                syn::Error::new(Span::call_site(), "missing `method = \"...\"` in attribute")
            })?,
        })
    }
}

fn take_attr_config(attrs: &mut Vec<Attribute>) -> syn::Result<Option<BuiltinKind>> {
    if let Some(idx) = attrs
        .iter()
        .position(|a| a.path().is_ident("boyia_async_builtin"))
    {
        let attr = attrs.remove(idx);
        return Ok(Some(BuiltinKind::Async(parse_async_config(&attr.meta)?)));
    }
    if let Some(idx) = attrs
        .iter()
        .position(|a| a.path().is_ident("boyia_sync_builtin"))
    {
        let attr = attrs.remove(idx);
        return Ok(Some(BuiltinKind::Sync(parse_sync_config(&attr.meta)?)));
    }
    Ok(None)
}

struct ArgInfo {
    name: syn::Ident,
    ty: Type,
    optional_default: Option<String>,
    /// 1-based script argument index.
    index: usize,
}

fn pat_ident(pat: &Pat) -> syn::Result<syn::Ident> {
    match pat {
        Pat::Ident(pi) => Ok(pi.ident.clone()),
        other => Err(syn::Error::new_spanned(
            other,
            "only simple identifier parameters are supported",
        )),
    }
}

fn parse_optional_default(attrs: &[syn::Attribute]) -> Option<String> {
    for attr in attrs {
        if !attr.path().is_ident("optional") {
            continue;
        }
        let mut default = None;
        if attr
            .parse_nested_meta(|meta| {
                if meta.path.is_ident("default") {
                    let value: LitStr = meta.value()?.parse()?;
                    default = Some(value.value());
                }
                Ok(())
            })
            .is_ok()
        {
            return default;
        }
    }
    None
}

fn collect_args(func: &ItemFn) -> syn::Result<(SelfArg, Vec<ArgInfo>)> {
    let mut self_arg = SelfArg::None;
    let mut args = Vec::new();
    let mut index = 1usize;
    for input in &func.sig.inputs {
        if let FnArg::Receiver(recv) = input {
            self_arg = if recv.mutability.is_some() {
                SelfArg::Mut
            } else {
                SelfArg::Ref
            };
            continue;
        }
        let FnArg::Typed(PatType { attrs, pat, ty, .. }) = input else {
            continue;
        };
        let name = pat_ident(pat)?;
        let optional_default = parse_optional_default(attrs);
        args.push(ArgInfo {
            name,
            ty: (**ty).clone(),
            optional_default,
            index,
        });
        index += 1;
    }
    Ok((self_arg, args))
}

fn type_last_ident(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(TypePath { path, .. }) => path.segments.last().map(|s| s.ident.to_string()),
        Type::Tuple(t) if t.elems.is_empty() => Some("()".into()),
        _ => None,
    }
}

fn is_option_string(ty: &Type) -> bool {
    let Type::Path(TypePath { path, .. }) = ty else {
        return false;
    };
    let Some(seg) = path.segments.last() else {
        return false;
    };
    if seg.ident != "Option" {
        return false;
    }
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
        return false;
    };
    args.args.len() == 1
        && args
            .args
            .iter()
            .next()
            .and_then(|a| match a {
                syn::GenericArgument::Type(inner) => type_last_ident(inner),
                _ => None,
            })
            .as_deref()
            == Some("String")
}

fn strip_optional_from_func(func: &mut ItemFn) {
    for input in &mut func.sig.inputs {
        if let FnArg::Typed(pt) = input {
            pt.attrs.retain(|a| !a.path().is_ident("optional"));
        }
    }
}

fn is_option_json_value(ty: &Type) -> bool {
    let Type::Path(TypePath { path, .. }) = ty else {
        return false;
    };
    let Some(seg) = path.segments.last() else {
        return false;
    };
    if seg.ident != "Option" {
        return false;
    }
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
        return false;
    };
    args.args.len() == 1
        && args
            .args
            .iter()
            .next()
            .and_then(|a| match a {
                syn::GenericArgument::Type(inner) => Some(is_json_value_type(inner)),
                _ => None,
            })
            == Some(true)
}

fn vec_element_type<'a>(ty: &'a Type) -> Option<&'a Type> {
    let Type::Path(TypePath { path, .. }) = ty else {
        return None;
    };
    let seg = path.segments.last()?;
    if seg.ident != "Vec" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
        return None;
    };
    match args.args.first()? {
        syn::GenericArgument::Type(inner) => Some(inner),
        _ => None,
    }
}

fn is_vec_usize(ty: &Type) -> bool {
    vec_element_type(ty).is_some_and(|inner| type_last_ident(inner).as_deref() == Some("usize"))
}

fn is_vec_nested_vec(ty: &Type) -> bool {
    vec_element_type(ty).is_some_and(|inner| type_last_ident(inner).as_deref() == Some("NestedVec"))
}

fn is_option_vec_usize(ty: &Type) -> bool {
    let Type::Path(TypePath { path, .. }) = ty else {
        return false;
    };
    let Some(seg) = path.segments.last() else {
        return false;
    };
    if seg.ident != "Option" {
        return false;
    }
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
        return false;
    };
    args.args.len() == 1
        && args
            .args
            .iter()
            .next()
            .and_then(|a| match a {
                syn::GenericArgument::Type(inner) => Some(is_vec_usize(inner)),
                _ => None,
            })
            == Some(true)
}

fn is_json_value_type(ty: &Type) -> bool {
    let Type::Path(TypePath { path, .. }) = ty else {
        return false;
    };
    let Some(last) = path.segments.last() else {
        return false;
    };
    if last.ident == "JsonValue" {
        return true;
    }
    last.ident == "Value"
        && path
            .segments
            .first()
            .is_some_and(|seg| seg.ident == "serde_json")
}

fn async_arg_extraction(arg: &ArgInfo) -> syn::Result<proc_macro2::TokenStream> {
    let name = &arg.name;
    let index = arg.index as i32;

    if is_json_value_type(&arg.ty) {
        return Ok(quote! {
            let #name = {
                let raw = crate::some_or_end!(site.arg_boyia_value(#index));
                crate::some_or_end!(unsafe {
                    crate::runner::builtin_json::boyia_value_to_json(site.vm(), raw).ok()
                })
            };
        });
    }

    if arg.optional_default.is_some() {
        let default = arg.optional_default.as_ref().unwrap();
        let default_lit = default.as_str();
        if type_last_ident(&arg.ty).as_deref() != Some("String") {
            return Err(syn::Error::new_spanned(
                &arg.ty,
                "`#[optional]` is only supported on `String` parameters",
            ));
        }
        return Ok(quote! {
            let #name = site.arg_string_or(#index, #default_lit);
        });
    }

    if type_last_ident(&arg.ty).as_deref() != Some("String") {
        return Err(syn::Error::new_spanned(
            &arg.ty,
            "async builtins support `String` or `serde_json::Value` parameters",
        ));
    }

    Ok(quote! {
        let #name = crate::some_or_end!(site.arg_string(#index));
    })
}

fn sync_arg_extraction(arg: &ArgInfo) -> syn::Result<proc_macro2::TokenStream> {
    let name = &arg.name;
    let index = arg.index as i32;

    if arg.optional_default.is_some() {
        let default = arg.optional_default.as_ref().unwrap();
        let default_lit = default.as_str();
        if type_last_ident(&arg.ty).as_deref() != Some("String") {
            return Err(syn::Error::new_spanned(
                &arg.ty,
                "`#[optional]` is only supported on `String` parameters",
            ));
        }
        return Ok(quote! {
            let #name = site.arg_string_or(#index, #default_lit);
        });
    }

    if is_option_string(&arg.ty) {
        return Ok(quote! {
            let #name = site.arg_string(#index);
        });
    }

    if is_vec_usize(&arg.ty) {
        return Ok(quote! {
            let #name = {
                let raw = crate::some_or_end!({
                    let val = unsafe {
                        boyia_vm::get_local_value(#index, site.vm()) as *const boyia_vm::BoyiaValue
                    };
                    if val.is_null() { None } else { Some(val) }
                });
                crate::some_or_end!(unsafe {
                    crate::runner::builtin_vec::boyia_value_to_vec_usize(site.vm(), raw).ok()
                })
            };
        });
    }

    if is_vec_nested_vec(&arg.ty) {
        return Ok(quote! {
            let #name = {
                let raw = crate::some_or_end!({
                    let val = unsafe {
                        boyia_vm::get_local_value(#index, site.vm()) as *const boyia_vm::BoyiaValue
                    };
                    if val.is_null() { None } else { Some(val) }
                });
                crate::some_or_end!(unsafe {
                    crate::runner::builtin_vec::boyia_value_to_nested_vec(site.vm(), raw).ok()
                })
            };
        });
    }

    if is_json_value_type(&arg.ty) {
        return Ok(quote! {
            let #name = {
                let raw = crate::some_or_end!({
                    let val = unsafe {
                        boyia_vm::get_local_value(#index, site.vm()) as *const boyia_vm::BoyiaValue
                    };
                    if val.is_null() { None } else { Some(val) }
                });
                crate::some_or_end!(unsafe {
                    crate::runner::builtin_json::boyia_value_to_json(site.vm(), raw).ok()
                })
            };
        });
    }

    let extract = match type_last_ident(&arg.ty).as_deref() {
        Some("String") => quote! { crate::some_or_end!(site.arg_string(#index)) },
        Some("bool") => quote! { crate::some_or_end!(site.arg_bool(#index)) },
        Some("i8" | "i16" | "i32") => quote! { crate::some_or_end!(site.arg_i32(#index)) },
        Some("i64" | "isize" | "u8" | "u16" | "u32" | "u64" | "usize") => {
            quote! { crate::some_or_end!(site.arg_i64(#index)) }
        }
        Some("f32" | "f64") => quote! { crate::some_or_end!(site.arg_f64(#index)) },
        Some(other) => {
            return Err(syn::Error::new_spanned(
                &arg.ty,
                format!(
                    "unsupported parameter type `{other}`; use String, bool, integer, float, Vec<usize>, Vec<NestedVec>, or Option<String>"
                ),
            ));
        }
        None => {
            return Err(syn::Error::new_spanned(
                &arg.ty,
                "unsupported parameter type",
            ));
        }
    };

    let cast = sync_arg_cast(&arg.ty, &extract);
    Ok(quote! {
        let #name = #cast;
    })
}

fn sync_arg_cast(ty: &Type, extract: &proc_macro2::TokenStream) -> proc_macro2::TokenStream {
    match type_last_ident(ty).as_deref() {
        Some("i8") => quote! { (#extract) as i8 },
        Some("i16") => quote! { (#extract) as i16 },
        Some("u8") => quote! { (#extract) as u8 },
        Some("u16") => quote! { (#extract) as u16 },
        Some("u32") => quote! { (#extract) as u32 },
        Some("u64") => quote! { (#extract) as u64 },
        Some("usize") => quote! { (#extract) as usize },
        Some("isize") => quote! { (#extract) as isize },
        Some("f32") => quote! { (#extract) as f32 },
        _ => quote! { #extract },
    }
}

fn validate_sync_return(ty: &Type) -> syn::Result<()> {
    if matches!(ty, Type::Tuple(t) if t.elems.is_empty()) {
        return Ok(());
    }
    if is_option_string(ty) {
        return Ok(());
    }
    if is_option_json_value(ty) {
        return Ok(());
    }
    if is_option_vec_usize(ty) {
        return Ok(());
    }
    match type_last_ident(ty).as_deref() {
        Some("bool" | "String" | "Handle" | "i8" | "i16" | "i32" | "i64" | "isize" | "u8" | "u16" | "u32" | "u64" | "usize" | "f32" | "f64") => {
            Ok(())
        }
        Some(other) => Err(syn::Error::new_spanned(
            ty,
            format!(
                "unsupported return type `{other}`; use (), bool, String, Option<String>, integer, or float"
            ),
        )),
        None => Err(syn::Error::new_spanned(ty, "unsupported return type")),
    }
}

/// VM native symbol: `{work_name}_native`, unless `native = ...` overrides.
fn resolve_native_name(work_name: &syn::Ident, override_native: Option<&syn::Ident>) -> syn::Ident {
    match override_native {
        Some(id) => id.clone(),
        None => format_ident!("{}_native", work_name),
    }
}

fn expand_async_method(
    config: &AsyncMethodConfig,
    func: &ItemFn,
    struct_ty: &Type,
) -> syn::Result<proc_macro2::TokenStream> {
    let (self_arg, args) = collect_args(func)?;
    if self_arg != SelfArg::None {
        return Err(syn::Error::new_spanned(
            func,
            "`self` on async builtins is not supported yet; use sync methods for field access",
        ));
    }
    let mut func = func.clone();
    strip_optional_from_func(&mut func);

    let work_name = &func.sig.ident;
    let schedule_name = format_ident!("schedule_{}", work_name);
    let handler_name = format_ident!("{}_handler", work_name);
    let native_name = resolve_native_name(work_name, config.native.as_ref());
    let required_args = args
        .iter()
        .filter(|a| a.optional_default.is_none())
        .count();
    let min_locals = (required_args + 2) as i32;
    let arg_names: Vec<_> = args.iter().map(|a| &a.name).collect();
    let arg_types: Vec<_> = args.iter().map(|a| &a.ty).collect();
    let arg_extractions: Vec<_> = args
        .iter()
        .map(async_arg_extraction)
        .collect::<Result<_, _>>()?;

    let before_cb = match &config.before {
        Some(path) => quote! { #path },
        None => quote! { |_| () },
    };

    Ok(quote! {
        fn #schedule_name(
            ctx: &crate::runner::r#async::AsyncCtx,
            #( #arg_names: #arg_types, )*
            callback: crate::runner::r#async::ScriptCallback,
        ) -> bool {
            ctx.spawn(
                move || #struct_ty::#work_name( #( #arg_names, )* ),
                callback,
                #before_cb,
            )
        }

        fn #handler_name(site: &mut crate::runner::r#async::CallSite<'_>) -> boyia_vm::OpHandleResult {
            #( #arg_extractions )*
            let callback = crate::some_or_end!(site.callback());
            site.finish(#schedule_name(site.ctx(), #( #arg_names, )* callback))
        }

        crate::define_async_native!(#native_name, #min_locals, #handler_name);
    })
}

fn expand_sync_method(
    config: &SyncMethodConfig,
    func: &ItemFn,
    struct_ty: &Type,
    has_native: bool,
) -> syn::Result<proc_macro2::TokenStream> {
    let (self_arg, args) = collect_args(func)?;
    if self_arg != SelfArg::None && !has_native {
        return Err(syn::Error::new_spanned(
            func,
            "`self` requires `#[boyia_class(..., native)]` + `#[boyia_native_object]`",
        ));
    }
    let mut func = func.clone();
    strip_optional_from_func(&mut func);

    let return_ty = match &func.sig.output {
        syn::ReturnType::Default => syn::parse_quote! { () },
        syn::ReturnType::Type(_, ty) => (**ty).clone(),
    };
    validate_sync_return(&return_ty)?;

    let work_name = &func.sig.ident;
    let handler_name = format_ident!("{}_handler", work_name);
    let native_name = resolve_native_name(work_name, config.native.as_ref());
    let required_args = args
        .iter()
        .filter(|a| a.optional_default.is_none())
        .count();
    let min_locals = if self_arg != SelfArg::None {
        (required_args + 2) as i32
    } else {
        (required_args + 1) as i32
    };
    let arg_names: Vec<_> = args.iter().map(|a| &a.name).collect();
    let arg_extractions: Vec<_> = args.iter().map(sync_arg_extraction).collect::<Result<_, _>>()?;

    let work_call = if has_native && self_arg != SelfArg::None {
        match self_arg {
            SelfArg::Ref | SelfArg::Mut => {
                quote! { #struct_ty::#work_name(state, #( #arg_names, )*) }
            }
            SelfArg::None => unreachable!(),
        }
    } else {
        match self_arg {
            SelfArg::None => quote! { #struct_ty::#work_name( #( #arg_names, )* ) },
            SelfArg::Ref => quote! { #struct_ty::#work_name(&state, #( #arg_names, )*) },
            SelfArg::Mut => quote! { #struct_ty::#work_name(&mut state, #( #arg_names, )*) },
        }
    };

    let (state_prelude, state_epilogue) = if has_native && self_arg != SelfArg::None {
        let access = match self_arg {
            SelfArg::Ref => quote! {
                boyia_gc::boyia_native_ref::<#struct_ty>(class_body, &mut *rt)
            },
            SelfArg::Mut => quote! {
                boyia_gc::boyia_native_mut::<#struct_ty>(class_body, &mut *rt)
            },
            SelfArg::None => unreachable!(),
        };
        (
            quote! {
                let class_body = crate::some_or_end!(site.this_function());
                let rt = unsafe { boyia_vm::get_runtime_from_vm(site.vm()) };
                if rt.is_null() {
                    return boyia_vm::OpHandleResult::kOpResultEnd;
                }
                let state = unsafe { #access };
            },
            quote! {},
        )
    } else {
        (quote! {}, quote! {})
    };

    let finish = if is_option_vec_usize(&return_ty) {
        quote! {
            let __sync_result = #work_call;
            #state_epilogue
            match __sync_result {
                Some(v) => crate::runner::builtin_vec::set_sync_vec_usize_return(v, site.vm()),
                None => boyia_vm::OpHandleResult::kOpResultEnd,
            }
        }
    } else if is_option_json_value(&return_ty) {
        quote! {
            let __sync_result = #work_call;
            #state_epilogue
            match __sync_result {
                Some(j) => crate::runner::builtin_json::set_sync_json_return(j, site.vm()),
                None => boyia_vm::OpHandleResult::kOpResultEnd,
            }
        }
    } else {
        quote! {
            let __sync_result = #work_call;
            #state_epilogue
            crate::runner::sync::set_sync_return(__sync_result, site.vm())
        }
    };

    Ok(quote! {
        fn #handler_name(site: &mut crate::runner::sync::SyncCallSite<'_>) -> boyia_vm::OpHandleResult {
            #state_prelude
            #( #arg_extractions )*
            #finish
        }

        crate::define_sync_native!(#native_name, #min_locals, #handler_name);
    })
}

fn impl_fn_to_item_fn(method: &ImplItemFn) -> syn::Result<ItemFn> {
    Ok(ItemFn {
        attrs: method.attrs.clone(),
        vis: method.vis.clone(),
        sig: method.sig.clone(),
        block: Box::new(method.block.clone()),
    })
}

fn expand_class(class_config: &ClassConfig, imp: &ItemImpl) -> syn::Result<proc_macro2::TokenStream> {
    if imp.trait_.is_some() {
        return Err(syn::Error::new_spanned(
            imp,
            "`#[boyia_class]` requires an inherent `impl Type { ... }`, not a trait impl",
        ));
    }

    let struct_ty = imp.self_ty.clone();
    let has_native = class_config.native;

    let mut method_expansions = Vec::new();
    let mut impl_methods = Vec::new();
    let mut attach_calls = Vec::new();

    for item in &imp.items {
        let ImplItem::Fn(method) = item else {
            return Err(syn::Error::new_spanned(
                item,
                "`#[boyia_class]` impl may only contain builtin functions",
            ));
        };
        let mut func = impl_fn_to_item_fn(method)?;
        let Some(kind) = take_attr_config(&mut func.attrs)? else {
            return Err(syn::Error::new_spanned(
                method,
                "functions inside `#[boyia_class]` must have `#[boyia_async_builtin(...)]` or `#[boyia_sync_builtin(...)]`",
            ));
        };

        let work_name = &func.sig.ident;
        let (method_lit, native_name) = match &kind {
            BuiltinKind::Async(cfg) => {
                method_expansions.push(expand_async_method(cfg, &func, &struct_ty)?);
                (
                    &cfg.method,
                    resolve_native_name(work_name, cfg.native.as_ref()),
                )
            }
            BuiltinKind::Sync(cfg) => {
                method_expansions.push(expand_sync_method(
                    cfg, &func, &struct_ty, has_native,
                )?);
                (
                    &cfg.method,
                    resolve_native_name(work_name, cfg.native.as_ref()),
                )
            }
        };

        strip_optional_from_func(&mut func);
        impl_methods.push(quote! { #func });

        attach_calls.push(quote! {
            crate::runner::r#async::attach_method(gen_id, #method_lit, #native_name, class_body, vm);
        });
    }

    if method_expansions.is_empty() {
        return Err(syn::Error::new_spanned(
            imp,
            "`#[boyia_class]` impl must contain at least one builtin function",
        ));
    }

    let class_name = &class_config.name;
    let registrar = &class_config.registrar;

    let attach_props = if has_native {
        quote! {
            unsafe { boyia_gc::attach_native_ptr_slot(class_body, gen_id); }
        }
    } else {
        quote! {}
    };

    Ok(quote! {
        impl #struct_ty {
            #( #impl_methods )*
        }

        #( #method_expansions )*

        pub fn #registrar(
            vm: &mut boyia_vm::BoyiaVM,
            gen_id: &mut dyn FnMut(&str) -> boyia_vm::LUintPtr,
        ) {
            crate::runner::r#async::register_async_builtin_class(vm, gen_id, #class_name, |class_body, vm, gen_id| {
                #attach_props
                #( #attach_calls )*
            });
        }
    })
}

// ---------------------------------------------------------------------------
// `#[boyia_native_object]` — struct fields in `Box<T>` with vtable GC (`nativePtr`)
// ---------------------------------------------------------------------------

fn parse_field_default(attrs: &[Attribute]) -> Option<String> {
    for attr in attrs {
        let is_default = attr.path().is_ident("boyia_field_default");
        if !is_default {
            continue;
        }
        if let Meta::NameValue(nv) = &attr.meta {
            if let Expr::Lit(ExprLit {
                lit: Lit::Str(s), ..
            }) = &nv.value
            {
                return Some(s.value());
            }
        }
    }
    None
}

fn field_default_init(field: &Field) -> syn::Result<proc_macro2::TokenStream> {
    let name = field.ident.as_ref().ok_or_else(|| {
        syn::Error::new_spanned(field, "tuple struct fields are not supported by `#[boyia_native_object]`")
    })?;
    let default_lit = parse_field_default(&field.attrs);
    let ty = &field.ty;
    let type_name = type_last_ident(ty).unwrap_or_default();

    let init = match type_name.as_str() {
        "bool" => {
            let v = default_lit.as_deref() == Some("true");
            quote! { #v }
        }
        "String" => {
            let v = default_lit.unwrap_or_default();
            quote! { #v.to_string() }
        }
        "f32" | "f64" => {
            let v: f64 = default_lit.as_deref().and_then(|s| s.parse().ok()).unwrap_or(0.0);
            quote! { #v as #ty }
        }
        "u64" | "usize" => {
            let v: u64 = default_lit.as_deref().and_then(|s| s.parse().ok()).unwrap_or(0);
            quote! { #v as #ty }
        }
        "i8" | "i16" | "i32" | "i64" | "isize" | "u8" | "u16" | "u32" => {
            let v: i64 = default_lit.as_deref().and_then(|s| s.parse().ok()).unwrap_or(0);
            quote! { #v as #ty }
        }
        other => {
            return Err(syn::Error::new_spanned(
                ty,
                format!(
                    "unsupported field type `{other}` in `#[boyia_native_object]`; \
                     use bool, integer types, float types, or String"
                ),
            ));
        }
    };

    Ok(quote! { #name: #init, })
}

fn expand_boyia_native_object(item: &ItemStruct) -> syn::Result<proc_macro2::TokenStream> {
    let struct_name = &item.ident;
    let generics = &item.generics;

    let fields = match &item.fields {
        Fields::Named(named) => &named.named,
        _ => {
            return Err(syn::Error::new_spanned(
                item,
                "`#[boyia_native_object]` requires a struct with named fields",
            ));
        }
    };

    if fields.is_empty() {
        return Err(syn::Error::new_spanned(
            item,
            "`#[boyia_native_object]` requires at least one field",
        ));
    }

    let mut struct_item = item.clone();
    struct_item.attrs.retain(|a| {
        !a.path().is_ident("boyia_native_object") && !a.path().is_ident("boyia_field_default")
    });
    struct_item.attrs.push(syn::parse_quote! { #[repr(C)] });

    for field in struct_item.fields.iter_mut() {
        field.attrs.retain(|a| !a.path().is_ident("boyia_field_default"));
    }

    let hdr_field: Field = syn::parse_quote! {
        __boyia_hdr: boyia_gc::NativePropHeader
    };
    if let Fields::Named(ref mut named) = struct_item.fields {
        named.named.insert(0, hdr_field);
    }

    let mut default_fields = Vec::new();
    for field in fields {
        default_fields.push(field_default_init(field)?);
    }

    Ok(quote! {
        #struct_item

        impl #struct_name #generics {
            fn boyia_new_header() -> boyia_gc::NativePropHeader {
                boyia_gc::NativePropHeader::new(&<Self as boyia_gc::NativePropTrait>::VTABLE)
            }
        }

        impl boyia_gc::NativePropTrait for #struct_name #generics {
            const VTABLE: boyia_gc::NativePropVTable = boyia_gc::NativePropVTable {
                mark_fn: Self::native_mark,
                flag_fn: Self::native_flag,
                drop_fn: Self::native_drop,
            };

            fn boyia_default() -> Self {
                Self {
                    __boyia_hdr: Self::boyia_new_header(),
                    #( #default_fields )*
                }
            }

            unsafe fn native_drop(ptr: *mut boyia_vm::LVoid) {
                let _ = Box::from_raw(ptr as *mut Self);
            }
        }
    })
}

/// ```ignore
/// #[boyia_native_object]
/// struct ConfigBuiltins {
///     #[boyia_field_default = "false"]
///     debug: bool,
///     timeout_ms: u64,
/// }
/// ```
#[proc_macro_attribute]
pub fn boyia_native_object(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let item = parse_macro_input!(item as ItemStruct);

    match expand_boyia_native_object(&item) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// ```ignore
/// struct FileBuiltins;
///
/// #[boyia_class(name = "File", registrar = builtin_file_class)]
/// impl FileBuiltins {
///     #[boyia_async_builtin(method = "read")]
///     fn file_read(path: String) -> AsyncBuiltinResult { ... }
///
///     #[boyia_sync_builtin(method = "isAbsolute")]
///     fn file_is_absolute(path: String) -> bool { ... }
/// }
/// ```
#[proc_macro_attribute]
pub fn boyia_class(attr: TokenStream, item: TokenStream) -> TokenStream {
    let class_config = parse_macro_input!(attr as ClassConfig);
    let imp = parse_macro_input!(item as ItemImpl);

    match expand_class(&class_config, &imp) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
