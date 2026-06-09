//! `#[boyia_async_class]` — compile-time class registrar with unrolled `attach_method` calls.
//! `#[boyia_async_builtin]` on child fns is parsed by the class macro (not a separate proc macro).

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{format_ident, quote};
use syn::{
    parse::Parse, parse_macro_input, token::Comma, Attribute, Expr, ExprLit, FnArg, Item, ItemFn,
    ItemMod, Lit, LitStr, Meta, Pat, PatType,
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

struct MethodConfig {
    native: syn::Ident,
    method: LitStr,
    before: Option<syn::Path>,
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

fn parse_method_config(meta: &Meta) -> syn::Result<MethodConfig> {
    let Meta::List(list) = meta else {
        return Err(syn::Error::new_spanned(
            meta,
            "expected `#[boyia_async_builtin(native = ..., method = \"...\")]`",
        ));
    };

    let config: MethodConfig = syn::parse2(list.tokens.clone())?;
    Ok(config)
}

impl Parse for MethodConfig {
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
                        "`class` belongs on `#[boyia_async_class]`, not on `#[boyia_async_builtin]`",
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

        Ok(MethodConfig {
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

fn take_boyia_async_builtin_attr(attrs: &mut Vec<Attribute>) -> syn::Result<Option<MethodConfig>> {
    let Some(idx) = attrs
        .iter()
        .position(|a| a.path().is_ident("boyia_async_builtin"))
    else {
        return Ok(None);
    };
    let attr = attrs.remove(idx);
    let meta = attr.meta;
    Ok(Some(parse_method_config(&meta)?))
}

struct ArgInfo {
    name: syn::Ident,
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
        let FnArg::Typed(PatType { attrs, pat, .. }) = input else {
            return Err(syn::Error::new_spanned(
                input,
                "`self` parameters are not supported",
            ));
        };
        let name = pat_ident(pat)?;
        let optional_default = parse_optional_default(attrs);
        args.push(ArgInfo {
            name,
            optional_default,
            index,
        });
        index += 1;
    }
    Ok(args)
}

fn expand_method(config: &MethodConfig, func: &ItemFn) -> syn::Result<proc_macro2::TokenStream> {
    let mut func = func.clone();
    for input in &mut func.sig.inputs {
        if let FnArg::Typed(pt) = input {
            pt.attrs.retain(|a| !a.path().is_ident("optional"));
        }
    }

    let work_name = &func.sig.ident;
    let schedule_name = format_ident!("schedule_{}", work_name);
    let handler_name = format_ident!("{}_handler", work_name);
    let native_name = &config.native;

    let args = collect_args(&func)?;
    let min_locals = (args.len() + 2) as i32;

    let arg_names: Vec<_> = args.iter().map(|a| &a.name).collect();

    let arg_extractions = args.iter().map(|a| {
        let name = &a.name;
        let index = a.index as i32;
        if let Some(default) = &a.optional_default {
            let default_lit = default.as_str();
            quote! {
                let #name = site.arg_string_or(#index, #default_lit);
            }
        } else {
            quote! {
                let #name = crate::some_or_end!(site.arg_string(#index));
            }
        }
    });

    let before_cb = match &config.before {
        Some(path) => quote! { #path },
        None => quote! { |_| () },
    };

    Ok(quote! {
        #func

        fn #schedule_name(
            ctx: &crate::runner::r#async::AsyncCtx,
            #( #arg_names: String, )*
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

fn expand_class(class_config: &ClassConfig, module: &ItemMod) -> syn::Result<proc_macro2::TokenStream> {
    let items = module
        .content
        .as_ref()
        .map(|(_, items)| items.as_slice())
        .ok_or_else(|| {
            syn::Error::new_spanned(module, "`#[boyia_async_class]` requires an inline module body")
        })?;

    let mut method_expansions = Vec::new();
    let mut attach_calls = Vec::new();

    for item in items {
        let Item::Fn(func) = item else {
            return Err(syn::Error::new_spanned(
                item,
                "`#[boyia_async_class]` module may only contain `#[boyia_async_builtin]` functions",
            ));
        };
        let mut func = func.clone();
        let Some(method_config) = take_boyia_async_builtin_attr(&mut func.attrs)? else {
            return Err(syn::Error::new_spanned(
                func,
                "functions inside `#[boyia_async_class]` must have `#[boyia_async_builtin(...)]`",
            ));
        };
        method_expansions.push(expand_method(&method_config, &func)?);
        let method_lit = &method_config.method;
        let native_name = &method_config.native;
        attach_calls.push(quote! {
            crate::runner::r#async::attach_method(gen_id, #method_lit, #native_name, class_body, vm);
        });
    }

    if method_expansions.is_empty() {
        return Err(syn::Error::new_spanned(
            module,
            "`#[boyia_async_class]` module must contain at least one `#[boyia_async_builtin]` function",
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
/// #[boyia_async_class(name = "File", registrar = builtin_file_class)]
/// mod file_builtins {
///     #[boyia_async_builtin(native = file_read_native, method = "read")]
///     fn file_read(path: String) -> AsyncBuiltinResult { ... }
/// }
/// ```
#[proc_macro_attribute]
pub fn boyia_async_class(attr: TokenStream, item: TokenStream) -> TokenStream {
    let class_config = parse_macro_input!(attr as ClassConfig);
    let module = parse_macro_input!(item as ItemMod);

    match expand_class(&class_config, &module) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
