// Copyright 2021 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use indexmap::set::IndexSet as HashSet;

use autocxx_parser::IncludeCppConfig;
use syn::ItemType;

use crate::{
    conversion::{
        analysis::type_converter::{add_analysis, Annotated, TypeConversionContext, TypeConverter},
        api::{AnalysisPhase, Api, ApiName, NullPhase, TypedefKind},
        apivec::ApiVec,
        convert_error::{ConvertErrorWithContext, ErrorContext},
        error_reporter::convert_apis,
        parse::BindgenSemanticAttributes,
        ConvertErrorFromCpp,
    },
    types::QualifiedName,
};

use super::{type_converter::TypedefTargetType, PointerTreatment};

#[derive(std::fmt::Debug)]
pub(crate) struct TypedefAnalysis {
    pub(crate) kind: TypedefKind,
    pub(crate) target: TypedefTargetType,
    pub(crate) deps: HashSet<QualifiedName>,
}

/// Analysis phase where typedef analysis has been performed but no other
/// analyses just yet.
#[derive(std::fmt::Debug)]
pub(crate) struct TypedefPhase;

impl AnalysisPhase for TypedefPhase {
    type TypedefAnalysis = TypedefAnalysis;
    type StructAnalysis = ();
    type FunAnalysis = ();
}

#[allow(clippy::needless_collect)] // we need the extra collect because the closure borrows extra_apis
pub(crate) fn convert_typedef_targets(
    config: &IncludeCppConfig,
    apis: ApiVec<NullPhase>,
) -> ApiVec<TypedefPhase> {
    let mut type_converter = TypeConverter::new(config, &apis);
    let mut extra_apis = ApiVec::new();
    let mut results = ApiVec::new();
    convert_apis(
        apis,
        &mut results,
        Api::fun_unchanged,
        Api::struct_unchanged,
        Api::enum_unchanged,
        |name, item, old_tyname, _| {
            Ok(Box::new(std::iter::once(match item {
                TypedefKind::Type(ity) => get_replacement_typedef(
                    name,
                    ity.into(),
                    old_tyname,
                    &mut type_converter,
                    &mut extra_apis,
                )?,
                TypedefKind::Use(ref _itu, ref boxed_type) => Api::Typedef {
                    name,
                    item: item.clone(),
                    old_tyname,
                    analysis: TypedefAnalysis {
                        kind: item.clone(),
                        target: TypedefTargetType {
                            ty: boxed_type.as_ref().clone(),
                            is_reference: false,
                        },
                        deps: HashSet::new(),
                    },
                },
            })))
        },
    );
    results.extend(extra_apis.into_iter().map(add_analysis));
    results
}

fn get_replacement_typedef(
    name: ApiName,
    ity: ItemType,
    old_tyname: Option<QualifiedName>,
    type_converter: &mut TypeConverter,
    extra_apis: &mut ApiVec<NullPhase>,
) -> Result<Api<TypedefPhase>, ConvertErrorWithContext> {
    if !ity.generics.params.is_empty() {
        return Err(ConvertErrorWithContext(
            ConvertErrorFromCpp::TypedefTakesGenericParameters,
            Some(ErrorContext::new_for_item(name.name.get_final_ident())),
        ));
    }
    let mut converted_type = ity.clone();
    let metadata = BindgenSemanticAttributes::new_retaining_others(&mut converted_type.attrs);
    metadata.check_for_fatal_attrs(&ity.ident)?;
    let is_reference = metadata.has_attr("reference");
    let type_conversion_context = if is_reference {
        TypeConversionContext::OuterType {
            pointer_treatment: PointerTreatment::Reference,
        }
    } else {
        TypeConversionContext::WithinReference
    };
    let type_conversion_results = type_converter.convert_type(
        (*ity.ty).clone(),
        name.name.get_namespace(),
        &type_conversion_context,
    );
    match type_conversion_results {
        Err(err) => Err(ConvertErrorWithContext(
            err,
            Some(ErrorContext::new_for_item(name.name.get_final_ident())),
        )),
        Ok(Annotated {
            ty: syn::Type::Path(ref typ),
            ..
        }) if QualifiedName::from_type_path(typ) == name.name => Err(ConvertErrorWithContext(
            ConvertErrorFromCpp::InfinitelyRecursiveTypedef(name.name.clone()),
            Some(ErrorContext::new_for_item(name.name.get_final_ident())),
        )),
        Ok(mut final_type) => {
            converted_type.ty = Box::new(final_type.ty.clone());
            extra_apis.append(&mut final_type.extra_apis);
            Ok(Api::Typedef {
                name,
                item: TypedefKind::Type(ity.into()),
                old_tyname,
                analysis: TypedefAnalysis {
                    kind: TypedefKind::Type(converted_type.into()),
                    target: TypedefTargetType {
                        ty: final_type.ty.into(),
                        is_reference: final_type.kind.is_reference(),
                    },
                    deps: final_type.types_encountered,
                },
            })
        }
    }
}
