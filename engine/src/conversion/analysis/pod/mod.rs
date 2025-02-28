// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

mod byvalue_checker;

use indexmap::map::IndexMap as HashMap;
use indexmap::set::IndexSet as HashSet;

use autocxx_parser::IncludeCppConfig;
use byvalue_checker::ByValueChecker;
use syn::{ItemStruct, Type, Visibility};

use crate::{
    conversion::{
        analysis::type_converter::{self, add_analysis, TypeConversionContext, TypeConverter},
        api::{AnalysisPhase, Api, ApiName, NullPhase, StructDetails, TypeKind},
        apivec::ApiVec,
        check_for_fatal_attrs,
        convert_error::{ConvertErrorWithContext, ErrorContext},
        error_reporter::convert_apis,
        ConvertErrorFromCpp,
    },
    types::{Namespace, QualifiedName},
    ParseCallbackResults,
};

use super::tdef::{TypedefAnalysis, TypedefPhase};

#[derive(std::fmt::Debug)]

pub(crate) struct FieldInfo {
    pub(crate) ty: Type,
    pub(crate) type_kind: type_converter::TypeKind,
}

#[derive(std::fmt::Debug)]
pub(crate) struct PodAnalysis {
    pub(crate) kind: TypeKind,
    pub(crate) bases: HashSet<QualifiedName>,
    /// Base classes for which we should create casts.
    /// That's just those which are on the allowlist,
    /// because otherwise we don't know whether they're
    /// abstract or not.
    pub(crate) castable_bases: HashSet<QualifiedName>,
    /// All field types. e.g. for std::unique_ptr<A>, this would include
    /// both std::unique_ptr and A
    pub(crate) field_deps: HashSet<QualifiedName>,
    /// Types within fields where we need a definition, e.g. for
    /// std::unique_ptr<A> it would just be std::unique_ptr.
    pub(crate) field_definition_deps: HashSet<QualifiedName>,
    pub(crate) field_info: Vec<FieldInfo>,
    pub(crate) num_generics: usize,
    pub(crate) in_anonymous_namespace: bool,
}

#[derive(std::fmt::Debug)]
pub(crate) struct PodPhase;

impl AnalysisPhase for PodPhase {
    type TypedefAnalysis = TypedefAnalysis;
    type StructAnalysis = PodAnalysis;
    type FunAnalysis = ();
}

/// In our set of APIs, work out which ones are safe to represent
/// by value in Rust (e.g. they don't have a destructor) and record
/// as such. Return a set of APIs annotated with extra metadata,
/// and an object which can be used to query the POD status of any
/// type whether or not it's one of the [Api]s.
pub(crate) fn analyze_pod_apis(
    apis: ApiVec<TypedefPhase>,
    config: &IncludeCppConfig,
    parse_callback_results: &ParseCallbackResults,
) -> Result<ApiVec<PodPhase>, ConvertErrorFromCpp> {
    // This next line will return an error if any of the 'generate_pod'
    // directives from the user can't be met because, for instance,
    // a type contains a std::string or some other type which can't be
    // held safely by value in Rust.
    let byvalue_checker = ByValueChecker::new_from_apis(&apis, config)?;
    let mut extra_apis = ApiVec::new();
    let mut type_converter = TypeConverter::new(config, &apis);
    let mut results = ApiVec::new();
    convert_apis(
        apis,
        &mut results,
        Api::fun_unchanged,
        |name, details, _| {
            analyze_struct(
                &byvalue_checker,
                &mut type_converter,
                &mut extra_apis,
                name,
                details,
                config,
                parse_callback_results,
            )
        },
        |name, item| analyze_enum(name, item, parse_callback_results),
        Api::typedef_unchanged,
    );
    // Conceivably, the process of POD-analysing the first set of APIs could result
    // in us creating new APIs to concretize generic types.
    let extra_apis: ApiVec<PodPhase> = extra_apis.into_iter().map(add_analysis).collect();
    let mut more_extra_apis = ApiVec::new();
    convert_apis(
        extra_apis,
        &mut results,
        Api::fun_unchanged,
        |name, details, _| {
            analyze_struct(
                &byvalue_checker,
                &mut type_converter,
                &mut more_extra_apis,
                name,
                details,
                config,
                parse_callback_results,
            )
        },
        |name, item| analyze_enum(name, item, parse_callback_results),
        Api::typedef_unchanged,
    );
    assert!(more_extra_apis.is_empty());
    Ok(results)
}

fn analyze_enum(
    name: ApiName,
    item: crate::minisyn::ItemEnum,
    parse_callback_results: &ParseCallbackResults,
) -> Result<Box<dyn Iterator<Item = Api<PodPhase>>>, ConvertErrorWithContext> {
    check_for_fatal_attrs(parse_callback_results, &name.name)?;
    Ok(Box::new(std::iter::once(Api::Enum { name, item })))
}

fn analyze_struct(
    byvalue_checker: &ByValueChecker,
    type_converter: &mut TypeConverter,
    extra_apis: &mut ApiVec<NullPhase>,
    name: ApiName,
    details: Box<StructDetails>,
    config: &IncludeCppConfig,
    parse_callback_results: &ParseCallbackResults,
) -> Result<Box<dyn Iterator<Item = Api<PodPhase>>>, ConvertErrorWithContext> {
    let id = name.name.get_final_ident();
    check_for_fatal_attrs(parse_callback_results, &name.name)?;
    let bases = get_bases(&details.item);
    let mut field_deps = HashSet::new();
    let mut field_definition_deps = HashSet::new();
    let mut field_info = Vec::new();
    let field_conversion_errors = get_struct_field_types(
        type_converter,
        name.name.get_namespace(),
        &details.item,
        &mut field_deps,
        &mut field_definition_deps,
        &mut field_info,
        extra_apis,
    );
    let type_kind = if byvalue_checker.is_pod(&name.name) {
        // It's POD so any errors encountered parsing its fields are important.
        // Let's not allow anything to be POD if it's got rvalue reference fields.
        if details.has_rvalue_reference_fields {
            return Err(ConvertErrorWithContext(
                ConvertErrorFromCpp::RValueReferenceField,
                Some(ErrorContext::new_for_item(id)),
            ));
        }
        if let Some(err) = field_conversion_errors.into_iter().next() {
            return Err(ConvertErrorWithContext(
                err,
                Some(ErrorContext::new_for_item(id)),
            ));
        }
        TypeKind::Pod
    } else {
        TypeKind::NonPod
    };
    let castable_bases = bases
        .iter()
        .filter(|(_, is_public)| **is_public)
        .map(|(base, _)| base)
        .filter(|base| config.is_on_allowlist(&base.to_cpp_name()))
        .cloned()
        .collect();
    let num_generics = details.item.generics.params.len();
    let in_anonymous_namespace = name
        .name
        .ns_segment_iter()
        .any(|ns| ns.starts_with("_bindgen_mod"));
    Ok(Box::new(std::iter::once(Api::Struct {
        name,
        details,
        analysis: PodAnalysis {
            kind: type_kind,
            bases: bases.into_keys().collect(),
            castable_bases,
            field_deps,
            field_definition_deps,
            field_info,
            num_generics,
            in_anonymous_namespace,
        },
    })))
}

fn get_struct_field_types(
    type_converter: &mut TypeConverter,
    ns: &Namespace,
    s: &ItemStruct,
    field_deps: &mut HashSet<QualifiedName>,
    field_definition_deps: &mut HashSet<QualifiedName>,
    field_info: &mut Vec<FieldInfo>,
    extra_apis: &mut ApiVec<NullPhase>,
) -> Vec<ConvertErrorFromCpp> {
    let mut convert_errors = Vec::new();
    let struct_type_params = s
        .generics
        .type_params()
        .map(|tp| tp.ident.clone())
        .collect();
    let type_conversion_context = TypeConversionContext::WithinStructField { struct_type_params };
    for f in &s.fields {
        let annotated = type_converter.convert_type(f.ty.clone(), ns, &type_conversion_context);
        match annotated {
            Ok(mut r) => {
                extra_apis.append(&mut r.extra_apis);
                // Skip base classes represented as fields. Anything which wants to include bases can chain
                // those to the list we're building.
                if !f
                    .ident
                    .as_ref()
                    .map(|id| {
                        id.to_string().starts_with("_base")
                            || id.to_string().starts_with("__bindgen_padding")
                    })
                    .unwrap_or(false)
                {
                    field_deps.extend(r.types_encountered);
                    if let Type::Path(typ) = &r.ty {
                        // Later analyses need to know about the field
                        // types where we need full definitions, as opposed
                        // to just declarations. That means just the outermost
                        // type path.
                        // TODO: consider arrays.
                        field_definition_deps.insert(QualifiedName::from_type_path(typ));
                    }
                    field_info.push(FieldInfo {
                        ty: r.ty,
                        type_kind: r.kind,
                    });
                }
            }
            Err(e) => convert_errors.push(e),
        };
    }
    convert_errors
}

/// Map to whether the bases are public.
fn get_bases(item: &ItemStruct) -> HashMap<QualifiedName, bool> {
    item.fields
        .iter()
        .filter_map(|f| {
            let is_public = matches!(f.vis, Visibility::Public(_));
            match &f.ty {
                Type::Path(typ) => f
                    .ident
                    .as_ref()
                    .filter(|id| id.to_string().starts_with("_base"))
                    .map(|_| (QualifiedName::from_type_path(typ), is_public)),
                _ => None,
            }
        })
        .collect()
}
