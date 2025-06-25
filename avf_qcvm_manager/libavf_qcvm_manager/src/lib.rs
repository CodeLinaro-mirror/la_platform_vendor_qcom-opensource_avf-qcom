/*
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries.
 * SPDX-License-Identifier: BSD-3-Clause-Clear */
use std::fmt::Debug;
use binder::{
    ExceptionCode, Status, Result as BinderResult
};
use log::warn;
pub mod dtbo;
pub mod avf_qcvm_manager;
pub mod virtual_machine;


/// Binder requires results to have a Binder Status, move the error handling
/// outside of HAL calls into other functions as much as possible.
/// Converts results to Binder results that implement Binder Status
pub fn to_binder_result<T, E: Debug>(result: Result<T, E>) -> BinderResult<T> {
    result.map_err(|e| {
        let message = format!("{:?}", e);
        warn!("Returning binder error: {}", &message);
        Status::new_exception_str(ExceptionCode::UNSUPPORTED_OPERATION, Some(message))
    })
}