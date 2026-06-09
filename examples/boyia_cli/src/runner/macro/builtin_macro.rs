//! `#[boyia_async_builtin]` — generate handler, schedule, and native shim from a work function.

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{format_ident, quote};
use syn::{
    parse_macro_input, token::Comma, Expr, FnArg, ItemFn, LitStr, Pat, PatType,
};

struct MacroConfig {
    native: syn::Ident,
    before: Option<syn::Path>,
}

impl syn::parse::Parse for MacroConfig {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut native = None;
        let mut before = None;

        while !input.is_empty() {
            let key: syn::Ident = input.parse()?;
            input.parse::<syn::Token![=]>()?;
            match key.to_string().as_str() {
                "native" => native = Some(input.parse()?),
                "before" => {
                    let expr: Expr = input.parse()?;
                    before = Some(expr_to_path(expr)?);
                }
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("unknown key `{other}`, expected `native` or `before`"),
                    ));
                }
            }
            if input.peek(Comma) {
                input.parse::<Comma>()?;
            }
        }

        Ok(MacroConfig {
            native: native.ok_or_else(|| {
                syn::Error::new(Span::call_site(), "missing `native = ...` in attribute")
            })?,
            before,
        })
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

/// ```ignore
/// #[boyia_async_builtin(native = file_delete_native)]
/// fn file_delete(path: String) -> AsyncBuiltinResult { ... }
///
/// #[boyia_async_builtin(native = zip_compress_native)]
/// fn zip_compress(
///     src: String,
///     dest: String,
///     #[optional(default = "")]
///     password: String,
/// ) -> AsyncBuiltinResult { ... }
/// ```
#[proc_macro_attribute]
pub fn boyia_async_builtin(attr: TokenStream, item: TokenStream) -> TokenStream {
    let config = parse_macro_input!(attr as MacroConfig);
    let func = parse_macro_input!(item as ItemFn);

    match expand(&config, &func) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn expand(config: &MacroConfig, func: &ItemFn) -> syn::Result<proc_macro2::TokenStream> {
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
