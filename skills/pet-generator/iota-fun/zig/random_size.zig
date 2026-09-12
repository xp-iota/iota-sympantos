const std = @import("std");

pub fn randomSize() []const u8 {
    const sizes = [_][]const u8{ "大", "中", "小" };
    var prng = std.Random.DefaultPrng.init(0xC0FFEE);
    const random = prng.random();
    return sizes[random.uintLessThan(usize, sizes.len)];
}
