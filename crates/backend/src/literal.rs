use ast;
use quote::{ToTokens, Tokens};
use shared;
use std::collections::BTreeSet;

pub struct LiteralBuilder<'a> {
    dst: &'a mut Tokens,
    cnt: usize,
}

impl<'a> LiteralBuilder<'a> {
    pub fn new(dst: &'a mut Tokens) -> LiteralBuilder<'a> {
        LiteralBuilder { dst, cnt: 0 }
    }

    pub fn finish(self) -> usize {
        self.cnt
    }

    fn byte(&mut self, b: u8) {
        ::syn::token::Comma::default().to_tokens(self.dst);
        self.cnt += 1;
        b.to_tokens(self.dst);
    }

    fn append(&mut self, s: &str) {
        for &b in s.as_bytes() {
            self.byte(b);
        }
    }

    fn str(&mut self, s: &str) {
        self.append("\"");
        self.append(s);
        self.append("\"");
    }

    fn bool(&mut self, v: bool) {
        if v {
            self.append("true")
        } else {
            self.append("false")
        }
    }

    fn u32(&mut self, s: u32) {
        self.append(&s.to_string())
    }

    fn as_char(&mut self, tokens: Tokens) {
        (quote! {
            ,(#tokens).__x[0]
            ,(#tokens).__x[1]
            ,(#tokens).__x[2]
            ,(#tokens).__x[3]
        }).to_tokens(self.dst);
        self.cnt += 4;
    }

    pub fn fields(&mut self, fields: &[(&str, &Fn(&mut Self))]) {
        self.append("{");
        for (i, &(field, cb)) in fields.iter().enumerate() {
            if i > 0 {
                self.append(",");
            }
            self.str(field);
            self.append(":");
            cb(self);
        }
        self.append("}");
    }

    pub fn list_of<'b, T, U>(&mut self, list: T)
    where
        T: IntoIterator<Item = &'b U>,
        U: 'b + Literal,
    {
        self.list(list, U::literal)
    }

    fn list<T, F>(&mut self, list: T, mut cb: F)
    where
        F: FnMut(T::Item, &mut Self),
        T: IntoIterator,
    {
        self.append("[");
        for (i, element) in list.into_iter().enumerate() {
            if i > 0 {
                self.append(",");
            }
            cb(element, self);
        }
        self.append("]");
    }
}

pub trait Literal {
    fn literal(&self, a: &mut LiteralBuilder);
}

impl Literal for ast::Program {
    fn literal(&self, a: &mut LiteralBuilder) {
        a.fields(&[
            ("exports", &|a| a.list_of(&self.exports)),
            ("imports", &|a| a.list_of(&self.imports)),
            ("enums", &|a| a.list_of(&self.enums)),
            ("custom_type_names", &|a| {
                let names = self.exports
                    .iter()
                    .filter_map(|e| e.class)
                    .chain(self.structs.iter().map(|s| s.name))
                    .collect::<BTreeSet<_>>();
                a.list(&names, |s, a| {
                    let val = shared::name_to_descriptor(s.as_ref());
                    a.fields(&[
                        ("descriptor", &|a| a.u32(val)),
                        ("name", &|a| a.str(s.as_ref())),
                    ]);
                })
            }),
            ("version", &|a| a.str(&shared::version())),
            ("schema_version", &|a| a.str(&shared::SCHEMA_VERSION)),
        ]);
    }
}

impl Literal for ast::Function {
    fn literal(&self, a: &mut LiteralBuilder) {
        a.fields(&[
            ("name", &|a| a.str(self.name.as_ref())),
            ("arguments", &|a| a.list_of(&self.arguments)),
            ("ret", &|a| match self.ret {
                Some(ref s) => s.literal(a),
                None => a.append("null"),
            }),
        ]);
    }
}

impl Literal for ast::Type {
    fn literal(&self, a: &mut LiteralBuilder) {
        let t = &self.ty;
        match self.kind {
            ast::TypeKind::ByValue => {
                a.as_char(quote! {
                    <#t as ::wasm_bindgen::convert::WasmBoundary>::DESCRIPTOR
                });
            }
            ast::TypeKind::ByRef |
            ast::TypeKind::ByMutRef => {
                match self.loc {
                    ast::TypeLocation::ImportArgument |
                    ast::TypeLocation::ExportRet => {
                        a.as_char(quote! {
                            <#t as ::wasm_bindgen::convert::ToRefWasmBoundary>
                                ::DESCRIPTOR
                        });
                    }
                    ast::TypeLocation::ImportRet |
                    ast::TypeLocation::ExportArgument => {
                        a.as_char(quote! {
                            <#t as ::wasm_bindgen::convert::FromRefWasmBoundary>
                                ::DESCRIPTOR
                        });
                    }
                }
            }
        }
    }
}

impl Literal for ast::Export {
    fn literal(&self, a: &mut LiteralBuilder) {
        a.fields(&[
            ("class", &|a| match self.class {
                Some(ref s) => a.str(s.as_ref()),
                None => a.append("null"),
            }),
            ("method", &|a| a.bool(self.method)),
            ("function", &|a| self.function.literal(a)),
        ]);
    }
}

impl Literal for ast::Import {
    fn literal(&self, a: &mut LiteralBuilder) {
        a.fields(&[
            ("module", &|a| match self.module {
                Some(ref s) => a.str(s),
                None => a.append("null"),
            }),
            ("js_namespace", &|a| match self.js_namespace {
                Some(ref s) => a.str(s.as_ref()),
                None => a.append("null"),
            }),
            ("kind", &|a| self.kind.literal(a)),
        ]);
    }
}

impl Literal for ast::ImportKind {
    fn literal(&self, a: &mut LiteralBuilder) {
        match *self {
            ast::ImportKind::Function(ref f) => f.literal(a),
            ast::ImportKind::Static(ref s) => s.literal(a),
            ast::ImportKind::Type(ref t) => t.literal(a),
        }
    }
}

impl Literal for ast::ImportFunction {
    fn literal(&self, a: &mut LiteralBuilder) {
        let mut method = false;
        let mut js_new = false;
        let mut class_name = None;
        match self.kind {
            ast::ImportFunctionKind::Method { ref class, .. } => {
                method = true;
                class_name = Some(class);
            }
            ast::ImportFunctionKind::JsConstructor { ref class, .. } => {
                js_new = true;
                class_name = Some(class);
            }
            ast::ImportFunctionKind::Normal => {}
        }

        let mut getter = None;
        let mut setter = None;
        let structural = self.function.opts.structural();

        if let Some(s) = self.function.opts.getter() {
            let s = s.map(|s| s.to_string());
            getter = Some(s.unwrap_or_else(|| self.infer_getter_property()));
        }
        if let Some(s) = self.function.opts.setter() {
            let s = s.map(|s| s.to_string());
            setter = Some(s.unwrap_or_else(|| self.infer_setter_property()));
        }
        a.fields(&[
            ("kind", &|a| a.str("function")),
            ("catch", &|a| a.bool(self.function.opts.catch())),
            ("method", &|a| a.bool(method)),
            ("js_new", &|a| a.bool(js_new)),
            ("structural", &|a| a.bool(structural)),
            ("shim", &|a| a.str(self.shim.as_ref())),
            ("getter", &|a| match getter {
                Some(ref s) => a.str(s),
                None => a.append("null"),
            }),
            ("setter", &|a| match setter {
                Some(ref s) => a.str(s),
                None => a.append("null"),
            }),
            ("function", &|a| self.function.literal(a)),
            ("class", &|a| match class_name {
                Some(s) => a.str(s),
                None => a.append("null"),
            }),
        ]);
    }
}

impl Literal for ast::Enum {
    fn literal(&self, a: &mut LiteralBuilder) {
        a.fields(&[
            ("name", &|a| a.str(self.name.as_ref())),
            ("variants", &|a| a.list_of(&self.variants)),
        ]);
    }
}

impl Literal for ast::Variant {
    fn literal(&self, a: &mut LiteralBuilder) {
        a.fields(&[
            ("name", &|a| a.str(self.name.as_ref())),
            ("value", &|a| a.append(&format!("{}", self.value))),
        ])
    }
}

impl Literal for ast::ImportStatic {
    fn literal(&self, a: &mut LiteralBuilder) {
        a.fields(&[
            ("kind", &|a| a.str("static")),
            ("name", &|a| a.str(self.js_name.as_ref())),
            ("shim", &|a| a.str(self.shim.as_ref())),
        ])
    }
}

impl Literal for ast::ImportType {
    fn literal(&self, a: &mut LiteralBuilder) {
        a.fields(&[("kind", &|a| a.str("type"))])
    }
}
