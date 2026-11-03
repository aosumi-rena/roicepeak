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
    let t0 = x8 - x25;
    let t1 = x24 + x9;
    let t2 = x25 + x8;
    let t3 = x9 - x24;
    let t4 = t2 - t3;
    let t5 = t3 + t2;
    let t6 = x2 - x19;
    let t7 = x18 + x3;
    let t8 = t7 * f32::from_bits(0x3e47c5c2);
    let t9 = t7 * f32::from_bits(0x3f7b14be);
    let t10 = t6 * f32::from_bits(0x3f7b14be);
    let t11 = t6 * f32::from_bits(0x3e47c5c2);
    let t12 = t10 - t8;
    let t13 = t11 + t9;
    let t14 = x7 - x22;
    let t15 = t14 * f32::from_bits(0x3f7b14be);
    let t16 = t14 * f32::from_bits(0x3e47c5c2);
    let t17 = x23 + x6;
    let t18 = t17 * f32::from_bits(0x3e47c5c2);
    let t19 = t17 * f32::from_bits(0x3f7b14be);
    let t20 = t15 + t18;
    let t21 = t16 - t19;
    let t22 = x10 - x27;
    let t23 = t22 * f32::from_bits(0x3f0e39da);
    let t24 = t22 * f32::from_bits(0x3f54db31);
    let t25 = x31 + x14;
    let t26 = t25 * f32::from_bits(0x3f0e39da);
    let t27 = t25 * f32::from_bits(0x3f54db31);
    let t28 = x6 - x23;
    let t29 = x4 - x21;
    let t30 = x22 + x7;
    let t31 = x20 + x5;
    let t32 = t30 * f32::from_bits(0x3f0e39da);
    let t33 = t30 * f32::from_bits(0x3f54db31);
    let t34 = t31 * f32::from_bits(0x3ec3ef15);
    let t35 = t31 * f32::from_bits(0x3f6c835e);
    let t36 = t28 * f32::from_bits(0x3f54db31);
    let t37 = t28 * f32::from_bits(0x3f0e39da);
    let t38 = t29 * f32::from_bits(0x3f6c835e);
    let t39 = t29 * f32::from_bits(0x3ec3ef15);
    let t40 = t36 - t32;
    let t41 = t38 - t34;
    let t42 = t37 + t33;
    let t43 = t39 + t35;
    let t44 = x15 - x30;
    let t45 = x30 + x15;
    let t46 = x28 + x13;
    let t47 = t44 * f32::from_bits(0x3f54db31);
    let t48 = t44 * f32::from_bits(0x3f0e39da);
    let t49 = x26 + x11;
    let t50 = t49 * f32::from_bits(0x3f54db31);
    let t51 = t49 * f32::from_bits(0x3f0e39da);
    let t52 = x19 + x2;
    let t53 = x21 + x4;
    let t54 = x3 - x18;
    let t55 = x5 - x20;
    let t56 = t26 - t47;
    let t57 = x1 - x16;
    let t58 = t54 * f32::from_bits(0x3f0e39da);
    let t59 = t54 * f32::from_bits(0x3f54db31);
    let t60 = t55 * f32::from_bits(0x3f6c835e);
    let t61 = t55 * f32::from_bits(0x3ec3ef15);
    let t62 = t52 * f32::from_bits(0x3f54db31);
    let t63 = t52 * f32::from_bits(0x3f0e39da);
    let t64 = t53 * f32::from_bits(0x3ec3ef15);
    let t65 = t53 * f32::from_bits(0x3f6c835e);
    let t66 = t48 + t27;
    let t67 = x0 + x17;
    let t68 = t62 - t58;
    let t69 = t64 - t60;
    let t70 = t63 + t59;
    let t71 = t65 + t61;
    let t72 = x11 - x26;
    let t73 = x29 + x12;
    let t74 = x13 - x28;
    let t75 = x27 + x10;
    let t76 = t75 * f32::from_bits(0x3f7b14be);
    let t77 = t75 * f32::from_bits(0x3e47c5c2);
    let t78 = t74 * f32::from_bits(0x3ec3ef15);
    let t79 = t74 * f32::from_bits(0x3f6c835e);
    let t80 = t72 * f32::from_bits(0x3e47c5c2);
    let t81 = t72 * f32::from_bits(0x3f7b14be);
    let t82 = t73 * f32::from_bits(0x3f6c835e);
    let t83 = t73 * f32::from_bits(0x3ec3ef15);
    let t84 = t80 + t76;
    let t85 = t83 + t79;
    let t86 = t81 - t77;
    let t87 = t82 - t78;
    let t88 = t68 - t84;
    let t89 = t70 - t86;
    let t90 = t69 - t87;
    let t91 = t71 - t85;
    let t92 = t84 + t68;
    let t93 = t86 + t70;
    let t94 = t87 + t69;
    let t95 = t85 + t71;
    let t96 = t4 * f32::from_bits(0x3f3504f3);
    let t97 = t5 * f32::from_bits(0x3f3504f3);
    let t98 = t57 - t96;
    let t99 = t67 + t97;
    let t100 = t67 - t97;
    let t101 = t20 + t56;
    let t102 = t21 + t66;
    let t103 = t96 + t57;
    let t104 = t23 - t50;
    let t105 = x0 - x17;
    let t106 = t51 + t24;
    let t107 = x1 + x16;
    let t108 = t45 * f32::from_bits(0x3f7b14be);
    let t109 = t45 * f32::from_bits(0x3e47c5c2);
    let t110 = t46 * f32::from_bits(0x3f6c835e);
    let t111 = t46 * f32::from_bits(0x3ec3ef15);
    let t112 = x14 - x31;
    let t113 = x12 - x29;
    let t114 = t112 * f32::from_bits(0x3e47c5c2);
    let t115 = t112 * f32::from_bits(0x3f7b14be);
    let t116 = t113 * f32::from_bits(0x3ec3ef15);
    let t117 = t113 * f32::from_bits(0x3f6c835e);
    let t118 = t114 - t108;
    let t119 = t116 - t110;
    let t120 = t115 + t109;
    let t121 = t117 + t111;
    let t122 = t0 - t1;
    let t123 = t1 + t0;
    let t124 = t122 * f32::from_bits(0x3f3504f3);
    let t125 = t123 * f32::from_bits(0x3f3504f3);
    let t126 = t12 + t104;
    let t127 = t13 + t106;
    let t128 = t124 + t105;
    let t129 = t125 + t107;
    let t130 = t12 - t104;
    let t131 = t13 - t106;
    let t132 = t105 - t124;
    let t133 = t107 - t125;
    let t134 = t118 + t40;
    let t135 = t120 + t42;
    let t136 = t119 + t41;
    let t137 = t121 + t43;
    let t138 = t40 - t118;
    let t139 = t42 - t120;
    let t140 = t41 - t119;
    let t141 = t43 - t121;
    let t142 = t134 + t126;
    let t143 = t135 + t127;
    let t144 = t136 + t128;
    let t145 = t137 + t129;
    let t146 = t142 + t144;
    let t147 = t143 + t145;
    let t148 = t144 - t142;
    let t149 = t145 - t143;
    let t150 = t126 - t134;
    let t151 = t127 - t135;
    let t152 = t128 - t136;
    let t153 = t129 - t137;
    let t154 = t152 - t151;
    let t155 = t153 - t150;
    let t156 = t153 + t150;
    let t157 = t152 + t151;
    let t158 = t131 - t138;
    let t159 = t130 - t139;
    let t160 = t131 + t138;
    let t161 = t130 + t139;
    let t162 = t132 - t141;
    let t163 = t133 - t140;
    let t164 = t141 + t132;
    let t165 = t140 + t133;
    let t166 = t159 - t160;
    let t167 = t160 + t159;
    let t168 = t166 * f32::from_bits(0x3f3504f3);
    let t169 = t167 * f32::from_bits(0x3f3504f3);
    let t170 = t168 + t162;
    let t171 = t169 + t165;
    let t172 = t162 - t168;
    let t173 = t165 - t169;
    let t174 = t161 - t158;
    let t175 = t174 * f32::from_bits(0x3f3504f3);
    let t176 = t161 + t158;
    let t177 = t176 * f32::from_bits(0x3f3504f3);
    let t178 = t164 - t177;
    let t179 = t163 - t175;
    let t180 = t163 + t175;
    let t181 = t164 + t177;
    let t182 = t88 - t101;
    let t183 = t89 - t102;
    let t184 = t90 + t100;
    let t185 = t91 + t103;
    let t186 = t182 + t184;
    let t187 = t183 + t185;
    let t188 = t184 - t182;
    let t189 = t185 - t183;
    let t190 = t100 - t90;
    let t191 = t103 - t91;
    let t192 = t101 + t88;
    let t193 = t102 + t89;
    let t194 = t190 - t193;
    let t195 = t191 - t192;
    let t196 = t191 + t192;
    let t197 = t190 + t193;
    let t198 = t99 - t95;
    let t199 = t94 + t98;
    let t200 = t95 + t99;
    let t201 = t98 - t94;
    let t202 = t20 - t56;
    let t203 = t21 - t66;
    let t204 = t203 + t92;
    let t205 = t93 - t202;
    let t206 = t92 - t203;
    let t207 = t205 + t204;
    let t208 = t204 - t205;
    let t209 = t208 * f32::from_bits(0x3f3504f3);
    let t210 = t207 * f32::from_bits(0x3f3504f3);
    let t211 = t209 + t198;
    let t212 = t210 + t199;
    let t213 = t198 - t209;
    let t214 = t199 - t210;
    let t215 = t93 + t202;
    let t216 = t206 - t215;
    let t217 = t216 * f32::from_bits(0x3f3504f3);
    let t218 = t206 + t215;
    let t219 = t218 * f32::from_bits(0x3f3504f3);
    let t220 = t200 - t219;
    let t221 = t201 - t217;
    let t222 = t201 + t217;
    let t223 = t200 + t219;
    [
        t146, t147, t148, t149, t154, t156, t157, t155, t170, t171, t172, t173, t178, t180, t181,
        t179, t186, t187, t188, t189, t194, t196, t197, t195, t211, t212, t213, t214, t220, t222,
        t223, t221,
    ]
}
