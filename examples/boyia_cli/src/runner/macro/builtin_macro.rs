//! `#[boyia_class]` — compile-time class registrar with unrolled `attach_method` calls.
//! Child fns use `#[boyia_async_builtin]` or `#[boyia_sync_builtin]` (parsed by this macro).

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{format_ident, quote};
use syn::{
    parse::Parse, parse_macro_input, token::Comma, Attribute, Expr, ExprLit, FnArg, ImplItem,
    ImplItemFn, ItemFn, ItemImpl, Lit, LitStr, Meta, Pat, PatType, Type, TypePath,
};

struct ClassConfig {
    name: LitStr,
    registrar: syn::Ident,
}

impl Parse for ClassConfig {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut name = None;
        let mut registrar = None;

        while !input.is_empty() {
            let key: syn::Ident = input.parse()?;
            input.parse::<syn::Token![=]>()?;
            match key.to_string().as_str() {
                "name" => name = Some(parse_string_lit(input)?),
                "registrar" => registrar = Some(input.parse()?),
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("unknown key `{other}`, expected `name` or `registrar`"),
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
        })
    }
}

struct AsyncMethodConfig {
    native: syn::Ident,
    method: LitStr,
    before: Option<syn::Path>,
}

struct SyncMethodConfig {
    native: syn::Ident,
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
            "expected `#[boyia_async_builtin(native = ..., method = \"...\")]`",
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
                            "unknown key `{other}`, expected `native`, `method`, or `before`"
                        ),
                    ));
                }
            }
            if input.peek(Comma) {
                input.parse::<Comma>()?;
            }
        }

        Ok(AsyncMethodConfig {
            native: native.ok_or_else(|| {
                syn::Error::new(Span::call_site(), "missing `native = ...` in attribute")
            })?,
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
            "expected `#[boyia_sync_builtin(native = ..., method = \"...\")]`",
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
                        format!("unknown key `{other}`, expected `native` or `method`"),
                    ));
                }
            }
            if input.peek(Comma) {
                input.parse::<Comma>()?;
            }
        }

        Ok(SyncMethodConfig {
            native: native.ok_or_else(|| {
                syn::Error::new(Span::call_site(), "missing `native = ...` in attribute")
            })?,
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

fn collect_args(func: &ItemFn) -> syn::Result<Vec<ArgInfo>> {
    let mut args = Vec::new();
    let mut index = 1usize;
    for input in &func.sig.inputs {
        let FnArg::Typed(PatType { attrs, pat, ty, .. }) = input else {
            return Err(syn::Error::new_spanned(
                input,
                "`self` parameters are not supported",
            ));
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
    Ok(args)
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

fn expand_async_method(config: &AsyncMethodConfig, func: &ItemFn) -> syn::Result<proc_macro2::TokenStream> {
    let args = collect_args(func)?;
    let mut func = func.clone();
    strip_optional_from_func(&mut func);

    let work_name = &func.sig.ident;
    let schedule_name = format_ident!("schedule_{}", work_name);
    let handler_name = format_ident!("{}_handler", work_name);
    let native_name = &config.native;
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
        #func

        fn #schedule_name(
            ctx: &crate::runner::r#async::AsyncCtx,
            #( #arg_names: #arg_types, )*
            callback: crate::runner::r#async::ScriptCallback,
        ) -> bool {
            ctx.spawn(
                move || #work_name( #( #arg_names, )* ),
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

fn expand_sync_method(config: &SyncMethodConfig, func: &ItemFn) -> syn::Result<proc_macro2::TokenStream> {
    let args = collect_args(func)?;
    let mut func = func.clone();
    strip_optional_from_func(&mut func);

    let return_ty = match &func.sig.output {
        syn::ReturnType::Default => syn::parse_quote! { () },
        syn::ReturnType::Type(_, ty) => (**ty).clone(),
    };
    validate_sync_return(&return_ty)?;

    let work_name = &func.sig.ident;
    let handler_name = format_ident!("{}_handler", work_name);
    let native_name = &config.native;
    let required_args = args
        .iter()
        .filter(|a| a.optional_default.is_none())
        .count();
    let min_locals = (required_args + 1) as i32;
    let arg_names: Vec<_> = args.iter().map(|a| &a.name).collect();
    let arg_extractions: Vec<_> = args.iter().map(sync_arg_extraction).collect::<Result<_, _>>()?;

    let finish = if is_option_vec_usize(&return_ty) {
        quote! {
            match #work_name( #( #arg_names, )* ) {
                Some(v) => crate::runner::builtin_vec::set_sync_vec_usize_return(v, site.vm()),
                None => boyia_vm::OpHandleResult::kOpResultEnd,
            }
        }
    } else if is_option_json_value(&return_ty) {
        quote! {
            match #work_name( #( #arg_names, )* ) {
                Some(j) => crate::runner::builtin_json::set_sync_json_return(j, site.vm()),
                None => boyia_vm::OpHandleResult::kOpResultEnd,
            }
        }
    } else {
        quote! {
            let result = #work_name( #( #arg_names, )* );
            crate::runner::sync::set_sync_return(result, site.vm())
        }
    };

    Ok(quote! {
        #func

        fn #handler_name(site: &mut crate::runner::sync::SyncCallSite<'_>) -> boyia_vm::OpHandleResult {
            #( #arg_extractions )*
            #finish
        }

        crate::define_sync_native!(#native_name, #min_locals, #handler_name);
    })
}

fn impl_fn_to_item_fn(method: &ImplItemFn) -> syn::Result<ItemFn> {
    if method.sig.receiver().is_some() {
        return Err(syn::Error::new_spanned(
            method.sig.receiver(),
            "`#[boyia_class]` methods must be associated functions without `self`",
        ));
    }
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

    let mut method_expansions = Vec::new();
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

        let (method_lit, native_name) = match &kind {
            BuiltinKind::Async(cfg) => {
                method_expansions.push(expand_async_method(cfg, &func)?);
                (&cfg.method, &cfg.native)
            }
            BuiltinKind::Sync(cfg) => {
                method_expansions.push(expand_sync_method(cfg, &func)?);
                (&cfg.method, &cfg.native)
            }
        };

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

    Ok(quote! {
        #( #method_expansions )*

        pub fn #registrar(
            vm: &mut boyia_vm::BoyiaVM,
            gen_id: &mut dyn FnMut(&str) -> boyia_vm::LUintPtr,
        ) {
            crate::runner::r#async::register_async_builtin_class(vm, gen_id, #class_name, |class_body, vm, gen_id| {
                #( #attach_calls )*
            });
        }
    })
}

/// ```ignore
/// struct FileBuiltins;
///
/// #[boyia_class(name = "File", registrar = builtin_file_class)]
/// impl FileBuiltins {
///     #[boyia_async_builtin(native = file_read_native, method = "read")]
///     fn file_read(path: String) -> AsyncBuiltinResult { ... }
///
///     #[boyia_sync_builtin(native = file_is_absolute_native, method = "isAbsolute")]
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
