//! Ordered scalar butterflies for sixteen complex values.

pub(super) fn apply(values: &[f32; 32]) -> [f32; 32] {
    let [
        x0,
        x1,
        x2,
        x3,
        x4,
        x5,
        x6,
        x7,
        x8,
        x9,
        x10,
        x11,
        x12,
        x13,
        x14,
        x15,
        x16,
        x17,
        x18,
        x19,
        x20,
        x21,
        x22,
        x23,
        x24,
        x25,
        x26,
        x27,
        x28,
        x29,
        x30,
        x31,
    ] = *values;
    let t0 = x16 + x0;
    let t1 = x17 + x1;
    let t2 = x18 + x2;
    let t3 = x19 + x3;
    let t4 = x24 + x8;
    let t5 = x25 + x9;
    let t6 = x26 + x10;
    let t7 = x27 + x11;
    let t8 = t4 + t0;
    let t9 = t5 + t1;
    let t10 = t6 + t2;
    let t11 = t7 + t3;
    let t12 = x20 + x4;
    let t13 = x21 + x5;
    let t14 = x22 + x6;
    let t15 = x23 + x7;
    let t16 = x4 - x20;
    let t17 = x5 - x21;
    let t18 = x6 - x22;
    let t19 = x7 - x23;
    let t20 = x12 - x28;
    let t21 = x13 - x29;
    let t22 = x14 - x30;
    let t23 = x15 - x31;
    let t24 = t17 - t20;
    let t25 = t16 - t21;
    let t26 = t20 + t17;
    let t27 = t21 + t16;
    let t28 = t25 - t26;
    let t29 = t26 + t25;
    let t30 = t27 + t24;
    let t31 = t24 - t27;
    let t32 = t30 * f32::from_bits(0x3f3504f3);
    let t33 = t31 * f32::from_bits(0x3f3504f3);
    let t34 = x28 + x12;
    let t35 = x29 + x13;
    let t36 = x30 + x14;
    let t37 = x31 + x15;
    let t38 = t0 - t4;
    let t39 = t1 - t5;
    let t40 = t2 - t6;
    let t41 = t3 - t7;
    let t42 = t14 - t36;
    let t43 = t15 - t37;
    let t44 = x2 - x18;
    let t45 = x3 - x19;
    let t46 = x11 - x27;
    let t47 = x10 - x26;
    let t48 = t44 - t46;
    let t49 = t18 - t23;
    let t50 = t45 - t47;
    let t51 = t47 + t45;
    let t52 = t22 + t19;
    let t53 = t46 + t44;
    let t54 = t51 * f32::from_bits(0x3ec3ef15);
    let t55 = t51 * f32::from_bits(0x3f6c835e);
    let t56 = t48 * f32::from_bits(0x3f6c835e);
    let t57 = t48 * f32::from_bits(0x3ec3ef15);
    let t58 = t56 - t54;
    let t59 = t57 + t55;
    let t60 = t52 * f32::from_bits(0x3f6c835e);
    let t61 = t52 * f32::from_bits(0x3ec3ef15);
    let t62 = x8 - x24;
    let t63 = x9 - x25;
    let t64 = t49 * f32::from_bits(0x3ec3ef15);
    let t65 = t49 * f32::from_bits(0x3f6c835e);
    let t66 = x0 - x16;
    let t67 = x1 - x17;
    let t68 = t64 - t60;
    let t69 = t66 - t63;
    let t70 = t67 - t62;
    let t71 = t65 + t61;
    let t72 = t66 + t63;
    let t73 = t67 + t62;
    let t74 = t23 + t18;
    let t75 = t19 - t22;
    let t76 = t50 * f32::from_bits(0x3f6c835e);
    let t77 = t50 * f32::from_bits(0x3ec3ef15);
    let t78 = t75 * f32::from_bits(0x3f6c835e);
    let t79 = t75 * f32::from_bits(0x3ec3ef15);
    let t80 = t53 * f32::from_bits(0x3ec3ef15);
    let t81 = t53 * f32::from_bits(0x3f6c835e);
    let t82 = t74 * f32::from_bits(0x3ec3ef15);
    let t83 = t74 * f32::from_bits(0x3f6c835e);
    let t84 = t80 - t76;
    let t85 = t83 - t79;
    let t86 = t81 + t77;
    let t87 = t82 + t78;
    let t88 = t84 - t85;
    let t89 = t86 - t87;
    let t90 = t86 + t87;
    let t91 = t84 + t85;
    let t92 = t72 - t32;
    let t93 = t70 - t33;
    let t94 = t72 + t32;
    let t95 = t70 + t33;
    let t96 = t92 - t88;
    let t97 = t93 - t89;
    let t98 = t94 - t90;
    let t99 = t95 - t91;
    let t100 = t92 + t88;
    let t101 = t93 + t89;
    let t102 = t94 + t90;
    let t103 = t95 + t91;
    let t104 = t28 * f32::from_bits(0x3f3504f3);
    let t105 = t29 * f32::from_bits(0x3f3504f3);
    let t106 = t58 + t68;
    let t107 = t59 + t71;
    let t108 = t104 + t69;
    let t109 = t105 + t73;
    let t110 = t106 + t108;
    let t111 = t107 + t109;
    let t112 = t108 - t106;
    let t113 = t109 - t107;
    let t114 = t69 - t104;
    let t115 = t58 - t68;
    let t116 = t59 - t71;
    let t117 = t73 - t105;
    let t118 = t114 - t116;
    let t119 = t117 - t115;
    let t120 = t117 + t115;
    let t121 = t114 + t116;
    let t122 = t40 - t43;
    let t123 = t42 + t41;
    let t124 = t122 - t123;
    let t125 = t123 + t122;
    let t126 = t124 * f32::from_bits(0x3f3504f3);
    let t127 = t125 * f32::from_bits(0x3f3504f3);
    let t128 = t43 + t40;
    let t129 = t41 - t42;
    let t130 = t128 - t129;
    let t131 = t129 + t128;
    let t132 = t34 + t12;
    let t133 = t35 + t13;
    let t134 = t36 + t14;
    let t135 = t37 + t15;
    let t136 = t130 * f32::from_bits(0x3f3504f3);
    let t137 = t131 * f32::from_bits(0x3f3504f3);
    let t138 = t12 - t34;
    let t139 = t13 - t35;
    let t140 = t38 - t139;
    let t141 = t39 - t138;
    let t142 = t139 + t38;
    let t143 = t138 + t39;
    let t144 = t126 + t140;
    let t145 = t127 + t143;
    let t146 = t140 - t126;
    let t147 = t143 - t127;
    let t148 = t142 - t137;
    let t149 = t141 - t136;
    let t150 = t141 + t136;
    let t151 = t142 + t137;
    let t152 = t132 + t8;
    let t153 = t133 + t9;
    let t154 = t134 + t10;
    let t155 = t135 + t11;
    let t156 = t154 + t152;
    let t157 = t155 + t153;
    let t158 = t152 - t154;
    let t159 = t153 - t155;
    let t160 = t8 - t132;
    let t161 = t9 - t133;
    let t162 = t10 - t134;
    let t163 = t11 - t135;
    let t164 = t160 - t163;
    let t165 = t161 - t162;
    let t166 = t161 + t162;
    let t167 = t160 + t163;
    [
        t156, t157, t158, t159, t164, t166, t167, t165, t144, t145, t146, t147, t148, t150, t151,
        t149, t110, t111, t112, t113, t118, t120, t121, t119, t100, t101, t96, t97, t98, t103,
        t102, t99,
    ]
}
