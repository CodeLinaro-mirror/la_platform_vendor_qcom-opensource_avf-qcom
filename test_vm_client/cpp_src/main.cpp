/* 
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries.
 * SPDX-License-Identifier: BSD-3-Clause-Clear
 */

#include <android/binder_manager.h>
#include <android/binder_process.h>
#include "VirtualMachineCallback.h"
#include <aidl/vendor/qti/AvfQcvmManager/IAvfQcvmManager.h>
#include <aidl/vendor/qti/AvfQcvmManager/IVirtualMachine.h>

using namespace aidl::vendor::qti::AvfQcvmManager;

int main() {

    std::string name =
        std::string(IAvfQcvmManager::descriptor)
        + "/default";

    /* Get a Binder Handle to AvfQcvmManager */
    ndk::SpAIBinder sysBinder = ndk::SpAIBinder(AServiceManager_waitForService(name.c_str()));

    /* Create  an IAvfQcvmManager Pointer*/
     std::shared_ptr<IAvfQcvmManager> server = IAvfQcvmManager::fromBinder(sysBinder);

    std::shared_ptr<IVirtualMachine> vm = nullptr;
    server->getVm("trustedvm", &vm);

    std::shared_ptr<IVirtualMachineCallback> vm_callback = ndk::SharedRefBase::make<VirtualMachineCallback>();
    vm->start(vm_callback);
    // /*
    //  * Starting the threadPool enables all the threads in libbinder
    //  * to be able to recieve calls.
    //  */
    // ABinderProcess_startThreadPool();
    /* Joins the current thread to the threadPool */
    ABinderProcess_joinThreadPool();

    /* This line should not be reached */
    return EXIT_FAILURE;
}