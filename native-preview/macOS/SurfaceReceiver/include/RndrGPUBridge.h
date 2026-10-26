#import <Foundation/Foundation.h>
#import <IOSurface/IOSurface.h>

NS_ASSUME_NONNULL_BEGIN
@interface RndrSurfaceReceiver : NSObject
- (BOOL)startWithDeviceID:(NSString *)deviceID componentID:(NSString *)componentID error:(NSError **)error;
@property(nonatomic, copy, nullable) void (^onSurface)(void);
- (void)stop;
- (nullable IOSurfaceRef)copyLatestSurface CF_RETURNS_RETAINED;
@property(atomic, readonly) NSUInteger receivedSurfaces;
@end
NS_ASSUME_NONNULL_END
