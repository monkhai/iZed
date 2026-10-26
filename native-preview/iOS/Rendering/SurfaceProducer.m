#import "SurfaceProducer.h"
#import <objc/message.h>
#import <dlfcn.h>
#import <mach/mach.h>
#import <mach/task_special_ports.h>
#import <IOSurface/IOSurfaceRef.h>

// Reuse the backing buffer until the component's pixel dimensions change.
static NSMutableDictionary<NSString *, id> *surfaces;

// The Mac receiver uses the same message layout and service name.
struct SurfaceMessage {
    mach_msg_header_t header;
    mach_msg_body_t body;
    mach_msg_port_descriptor_t surface;
    uint32_t width, height;
};

NSDictionary *RndrRenderSharedSurface(CALayer *layer, CGFloat scale, NSString *componentID) {
    if (!surfaces) surfaces = [NSMutableDictionary dictionary];
    IOSurfaceRef componentSurface = (__bridge IOSurfaceRef)surfaces[componentID];
    size_t width = (size_t)ceil(layer.bounds.size.width * scale);
    size_t height = (size_t)ceil(layer.bounds.size.height * scale);
    if (!componentSurface || IOSurfaceGetWidth(componentSurface) != width || IOSurfaceGetHeight(componentSurface) != height) {
        size_t row = IOSurfaceAlignProperty(kIOSurfaceBytesPerRow, width * 4);
        IOSurfaceRef newSurface = IOSurfaceCreate((__bridge CFDictionaryRef)@{
            (__bridge NSString *)kIOSurfaceWidth: @(width),
            (__bridge NSString *)kIOSurfaceHeight: @(height),
            (__bridge NSString *)kIOSurfaceBytesPerElement: @4,
            (__bridge NSString *)kIOSurfaceBytesPerRow: @(row),
            (__bridge NSString *)kIOSurfaceAllocSize: @(row * height),
            (__bridge NSString *)kIOSurfacePixelFormat: @((uint32_t)'BGRA')
        });
        if (!newSurface) return @{@"error": @"IOSurfaceCreate failed"};
        surfaces[componentID] = CFBridgingRelease(newSurface);
        componentSurface = (__bridge IOSurfaceRef)surfaces[componentID];
    }
    if (!componentSurface) return @{@"error": @"IOSurfaceCreate failed"};

    // Private Apple API: render the existing component layer subtree into our buffer.
    id context = ((id (*)(id, SEL))objc_msgSend)(layer, NSSelectorFromString(@"context"));
    uint32_t contextID = [[context valueForKey:@"contextId"] unsignedIntValue];
    void (*render)(mach_port_t, uint32_t, uint64_t, IOSurfaceRef, int32_t, int32_t, const CATransform3D *) = dlsym(RTLD_DEFAULT, "CARenderServerRenderLayerWithTransform");
    if (!render) return @{@"error": @"Render-server surface function missing"};
    CATransform3D transform = CATransform3DMakeScale(scale, scale, 1);
    render(MACH_PORT_NULL, contextID, (uint64_t)(__bridge void *)layer, componentSurface, 0, 0, &transform);

    // Send permission to access the buffer, rather than encoded image bytes.
    mach_port_t bootstrap = MACH_PORT_NULL, destination = MACH_PORT_NULL;
    task_get_bootstrap_port(mach_task_self(), &bootstrap);
    kern_return_t (*lookup)(mach_port_t, const char *, mach_port_t *) = dlsym(RTLD_DEFAULT, "bootstrap_look_up");
    kern_return_t result = lookup ? lookup(bootstrap, [[@"dev.ized.catalog.component." stringByAppendingString:componentID] UTF8String], &destination) : -1;
    if (!result) {
        mach_port_t surfacePort = IOSurfaceCreateMachPort(componentSurface);
        struct SurfaceMessage message = {0};
        message.header.msgh_bits = MACH_MSGH_BITS_COMPLEX | MACH_MSGH_BITS(MACH_MSG_TYPE_COPY_SEND, 0);
        message.header.msgh_size = sizeof(message);
        message.header.msgh_remote_port = destination;
        message.header.msgh_id = 0x524e4452;
        message.body.msgh_descriptor_count = 1;
        message.surface.name = surfacePort;
        message.surface.disposition = MACH_MSG_TYPE_COPY_SEND;
        message.surface.type = MACH_MSG_PORT_DESCRIPTOR;
        message.width = (uint32_t)width;
        message.height = (uint32_t)height;
        result = mach_msg(&message.header, MACH_SEND_MSG | MACH_SEND_TIMEOUT, sizeof(message), 0, MACH_PORT_NULL, 10, MACH_PORT_NULL);
        mach_port_deallocate(mach_task_self(), surfacePort);
    }
    if (destination) mach_port_deallocate(mach_task_self(), destination);
    if (bootstrap) mach_port_deallocate(mach_task_self(), bootstrap);
    return @{@"surfaceID": @(IOSurfaceGetID(componentSurface)), @"width": @(width), @"height": @(height),
             @"machSendResult": @(result),
             @"transport": @"CA render server -> shared IOSurface -> Mach right -> Metal"};
}
