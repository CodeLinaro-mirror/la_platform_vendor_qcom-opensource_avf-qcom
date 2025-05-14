/*
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries.
 * SPDX-License-Identifier: BSD-3-Clause-Clear */
#![allow(non_snake_case)]
#![allow(unused_imports)]
#![allow(dead_code)]
#![allow(unused_mut)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(unused_variables)]

pub mod dtbo;
pub mod avf_qcvm_manager;
pub mod virtual_machine;

use avf_qcvm_manager::*;
use log::LevelFilter;
use std::env;
use log::{warn, info, error};



fn main() {
    binder::ProcessState::set_thread_pool_max_thread_count(12);
    binder::ProcessState::start_thread_pool();

    let args: Vec<String> = env::args().collect();
    let log_level = if args.get(1) == Some(&"-v".to_string()) {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    };
    let _init_success = logger::init(
        logger::Config::default()
            .with_tag_on_device("avf_qcvm_manager_rs")
            .with_max_level(log_level),
    );

    let avf_qcvm_manager = AvfQcvmManager::new();
    let avf_qcvm_manager_bndr = avf_qcvm_manager.expect("Failed to create Binder intstance").to_binder();
    let descriptor = AvfQcvmManager::get_descriptor();
    binder::add_service(
        &format!("{}/default", descriptor),
        avf_qcvm_manager_bndr.as_binder(),
    )
    .expect("Failed to register service.");

    // Do not return
    binder::ProcessState::join_thread_pool()
}
