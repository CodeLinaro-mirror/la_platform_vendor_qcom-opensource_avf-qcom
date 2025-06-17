/*
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries.
 * SPDX-License-Identifier: BSD-3-Clause-Clear
 */
#include <aidl/vendor/qti/AvfQcvmManager/BnVirtualMachineCallback.h>
#include <aidl/vendor/qti/AvfQcvmManager/VirtualMachineError.h>

#include <atomic>
#include <memory>

enum class VmState {
    NotStarted,
    Started,
    UserspaceReady,
    ShutdownInitiated,
    Crashed,
    Stopped,
    Error
};


class VirtualMachineCallback : public aidl::vendor::qti::AvfQcvmManager::BnVirtualMachineCallback{
    public:
        VirtualMachineCallback(std::shared_ptr<std::atomic<VmState>> state);
        std::string static name;
        ::ndk::ScopedAStatus onStarting() override;
        ::ndk::ScopedAStatus onUserspaceReady() override;
        ::ndk::ScopedAStatus onShutdownInitiated() override;
        ::ndk::ScopedAStatus onCrashed() override;
        ::ndk::ScopedAStatus onStopped() override;
        ::ndk::ScopedAStatus onError(aidl::vendor::qti::AvfQcvmManager::VirtualMachineError in_error) override;
    private:
        std::shared_ptr<std::atomic<VmState>> vm_state_;
};