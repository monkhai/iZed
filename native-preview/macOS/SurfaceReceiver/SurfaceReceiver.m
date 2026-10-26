#import "RndrGPUBridge.h"
#import <dlfcn.h>
#import <mach/mach.h>

// Private CoreSimulator methods used to expose a Mach endpoint inside a simulator.
@interface NSObject (RndrSimPrivate)
+ (id)sharedServiceContextForDeveloperDir:(NSString *)path error:(NSError **)error;
- (id)defaultDeviceSetWithError:(NSError **)error;
- (BOOL)registerPort:(mach_port_t)port service:(NSString *)service error:(NSError **)error;
- (BOOL)unregisterService:(NSString *)service error:(NSError **)error;
@end

// Must match iOS/Rendering/SurfaceProducer.m. The message carries a handle, not pixels.
struct SurfaceMessage {
    mach_msg_header_t header;
    mach_msg_body_t body;
    mach_msg_port_descriptor_t surface;
    uint32_t width, height;
};

@interface RndrSurfaceReceiver () {
    IOSurfaceRef _surface;
    mach_port_t _port;
    id _device;
    NSString *_service;
    BOOL _active;
    dispatch_source_t _source;
    NSUInteger _receivedSurfaces;
}
@end

@implementation RndrSurfaceReceiver

- (BOOL)startWithDeviceID:(NSString *)deviceID componentID:(NSString *)componentID error:(NSError **)error {
    _service = [@"dev.ized.catalog.component." stringByAppendingString:componentID];
    dlopen("/Library/Developer/PrivateFrameworks/CoreSimulator.framework/CoreSimulator", RTLD_NOW);
    id context = [NSClassFromString(@"SimServiceContext")
        sharedServiceContextForDeveloperDir:@"/Applications/Xcode.app/Contents/Developer" error:error];
    id deviceSet = [context defaultDeviceSetWithError:error];
    for (id candidate in [deviceSet valueForKey:@"devices"]) {
        if ([[[candidate valueForKey:@"UDID"] UUIDString] isEqualToString:deviceID]) {
            _device = candidate;
            break;
        }
    }
    if (!_device) {
        if (error) {
            *error = [NSError errorWithDomain:@"RndrGPU" code:1
                userInfo:@{NSLocalizedDescriptionKey: @"Simulator device not found"}];
        }
        return NO;
    }

    // A receive right stays in this process. A send right is registered with the simulator.
    kern_return_t result = mach_port_allocate(mach_task_self(), MACH_PORT_RIGHT_RECEIVE, &_port);
    if (!result) {
        result = mach_port_insert_right(mach_task_self(), _port, _port, MACH_MSG_TYPE_MAKE_SEND);
    }
    if (result) {
        if (_port) mach_port_mod_refs(mach_task_self(), _port, MACH_PORT_RIGHT_RECEIVE, -1);
        if (error) *error = [NSError errorWithDomain:NSMachErrorDomain code:result userInfo:nil];
        return NO;
    }
    [_device unregisterService:_service error:nil];
    if (![_device registerPort:_port service:_service error:error]) {
        mach_port_deallocate(mach_task_self(), _port);
        mach_port_mod_refs(mach_task_self(), _port, MACH_PORT_RIGHT_RECEIVE, -1);
        return NO;
    }

    @synchronized (self) { _active = YES; }
    // Event-driven reception avoids reserving 100 permanently blocked worker threads.
    _source = dispatch_source_create(DISPATCH_SOURCE_TYPE_MACH_RECV, _port, 0,
        dispatch_get_global_queue(QOS_CLASS_USER_INTERACTIVE, 0));
    dispatch_source_set_event_handler(_source, ^{ [self receiveSurfaces]; });
    mach_port_t port = _port;
    dispatch_source_set_cancel_handler(_source, ^{
        mach_port_deallocate(mach_task_self(), port);
        mach_port_mod_refs(mach_task_self(), port, MACH_PORT_RIGHT_RECEIVE, -1);
    });
    dispatch_resume(_source);
    return YES;
}

- (void)receiveSurfaces {
    while (YES) {
        @synchronized (self) { if (!_active) break; }
        struct { struct SurfaceMessage message; uint8_t trailer[512]; } buffer = {0};
        kern_return_t result = mach_msg(
            &buffer.message.header, MACH_RCV_MSG | MACH_RCV_TIMEOUT,
            0, sizeof(buffer), _port, 0, MACH_PORT_NULL);
        if (result == MACH_RCV_TIMED_OUT) break;
        if (result) break;

        struct SurfaceMessage *message = &buffer.message;
        BOOL valid = message->header.msgh_id == 0x524e4452
            && (message->header.msgh_bits & MACH_MSGH_BITS_COMPLEX)
            && message->body.msgh_descriptor_count == 1
            && message->surface.type == MACH_MSG_PORT_DESCRIPTOR;
        if (!valid) {
            mach_msg_destroy(&message->header);
            continue;
        }

        // Opening the handle maps the iOS-owned backing buffer into this process.
        IOSurfaceRef surface = IOSurfaceLookupFromMachPort(message->surface.name);
        mach_port_deallocate(mach_task_self(), message->surface.name);
        if (surface) {
            @synchronized (self) {
                if (_surface) CFRelease(_surface);
                _surface = surface;
                _receivedSurfaces++;
            }
            dispatch_async(dispatch_get_main_queue(), ^{
                if (self.onSurface) self.onSurface();
            });
        }
    }
}

- (IOSurfaceRef)copyLatestSurface {
    @synchronized (self) { return _surface ? (IOSurfaceRef)CFRetain(_surface) : nil; }
}

- (NSUInteger)receivedSurfaces {
    @synchronized (self) { return _receivedSurfaces; }
}

- (void)stop {
    @synchronized (self) { _active = NO; }
    [_device unregisterService:_service error:nil];
    if (_source) { dispatch_source_cancel(_source); _source = nil; }
}

- (void)dealloc {
    if (_surface) CFRelease(_surface);
}

@end
