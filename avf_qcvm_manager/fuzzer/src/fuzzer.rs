/*
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries.
 * SPDX-License-Identifier: BSD-3-Clause-Clear */
#![no_main]

use avf_qcvm_manager_rs::avf_qcvm_manager::AvfQcvmManager;
extern crate libfuzzer_sys;
use binder_random_parcel_rs::fuzz_service;

fuzz_target!(|data: &[u8]| {
    let service = AvfQcvmManager::new();
    let avf_qcvm_manager_bndr = service.expect("Failed to create Binder intstance").to_binder();
    fuzz_service(&mut avf_qcvm_manager_bndr.as_binder(), data);
});
