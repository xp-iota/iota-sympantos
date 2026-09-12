const random_size = @import("random_size.zig");

pub fn main() void {
    const val = random_size.randomSize();
    std.debug.print("{s}\n", .{val});
}
