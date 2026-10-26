#import <Foundation/Foundation.h>
#import <QuartzCore/QuartzCore.h>

/// Render one native layer subtree into a shared IOSurface and send its Mach handle to Rndr.
NSDictionary *RndrRenderSharedSurface(CALayer *layer, CGFloat scale, NSString *componentID);
