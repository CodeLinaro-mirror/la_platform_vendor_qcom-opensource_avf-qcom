/*
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries.
 * SPDX-License-Identifier: BSD-3-Clause-Clear */
use anyhow::{anyhow, Result};
use std::sync::{Arc, Mutex};

use guest_agent_client::{mink::MinkClient, vsock::VsockClient, GuestAgentClient,
    IGuestNotificationCallback, ServiceId};

pub struct GuestClient {
    pub guest_agent_client: Box<dyn GuestAgentClient>,
}

unsafe impl Send for GuestClient{}

impl GuestAgentClient for GuestClient {
    fn connect_userspace(retry: u32, start_vm_timer: u32, timeout: u32, service_id: ServiceId, guest_callback: Option<Arc<Mutex<dyn IGuestNotificationCallback + Send + Sync>>>) -> Result<Self> {
        if let ServiceId::VsockPort(_) = service_id {
            let vsock_client = match VsockClient::connect_userspace(retry, start_vm_timer, timeout, service_id, guest_callback) {
                Ok(client) => Box::new(client),
                Err(e) => {
                    return Err(anyhow!("{:?}", e));
                }
            };
            return Ok(Self {
                guest_agent_client: vsock_client,
            });
        }
        else{
            let mink_client = match MinkClient::connect_userspace(retry, start_vm_timer, timeout, service_id, guest_callback) {
                Ok(client) => Box::new(client),
                Err(e) => {
                    return Err(anyhow!("{:?}", e));
                }
            };
            return Ok(Self {
                guest_agent_client: mink_client,
            });
        }
    }

    fn shutdown(&self) -> Result<()> {
        self.guest_agent_client.shutdown()
    }
}
