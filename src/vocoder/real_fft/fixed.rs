//! Fixed eight-complex butterflies with individually rounded arithmetic.

pub(super) fn ordinary(values: &[f32; 16]) -> [f32; 16] {
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
    ] = *values;
    let h = std::f32::consts::FRAC_1_SQRT_2;
    let t0 = x10 + x2;
    let t1 = x14 + x6;
    let t2 = t0 + t1;
    let t3 = x12 + x4;
    let t4 = x0 + x8;
    let t5 = t3 + t4;
    let t6 = t2 + t5;
    let t7 = x11 + x3;
    let t8 = x15 + x7;
    let t9 = t7 + t8;
    let t10 = x13 + x5;
    let t11 = x1 + x9;
    let t12 = t10 + t11;
    let t13 = t9 + t12;
    let t14 = t5 - t2;
    let t15 = t12 - t9;
    let t16 = t4 - t3;
    let t17 = t7 - t8;
    let t18 = t16 - t17;
    let t19 = t11 - t10;
    let t20 = t0 - t1;
    let t21 = t19 + t20;
    let t22 = t16 + t17;
    let t23 = t19 - t20;
    let t24 = x2 - x10;
    let t25 = x7 - x15;
    let t26 = t24 - t25;
    let t27 = x6 - x14;
    let t28 = x3 - x11;
    let t29 = t27 + t28;
    let t30 = t26 - t29;
    let t31 = t30 * h;
    let t32 = x0 - x8;
    let t33 = x5 - x13;
    let t34 = t32 - t33;
    let t35 = t31 + t34;
    let t36 = t26 + t29;
    let t37 = t36 * h;
    let t38 = x4 - x12;
    let t39 = x1 - x9;
    let t40 = t38 + t39;
    let t41 = t37 + t40;
    let t42 = t34 - t31;
    let t43 = t40 - t37;
    let t44 = t32 + t33;
    let t45 = t28 - t27;
    let t46 = t25 + t24;
    let t47 = t45 + t46;
    let t48 = t47 * h;
    let t49 = t44 - t48;
    let t50 = t39 - t38;
    let t51 = t46 - t45;
    let t52 = t51 * h;
    let t53 = t50 + t52;
    let t54 = t44 + t48;
    let t55 = t50 - t52;
    [
        t6, t13, t14, t15, t18, t21, t22, t23, t35, t41, t42, t43, t49, t53, t54, t55,
    ]
}

pub(super) fn twisted(values: &[f32; 16]) -> [f32; 16] {
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
    ] = *values;
    let h = std::f32::consts::FRAC_1_SQRT_2;
    let s = f32::from_bits(0x3ec3ef15);
    let c = f32::from_bits(0x3f6c835e);
    let t0 = x2 - x11;
    let t1 = t0 * c;
    let t2 = x10 + x3;
    let t3 = t2 * s;
    let t4 = t1 - t3;
    let t5 = x6 - x15;
    let t6 = t5 * s;
    let t7 = x14 + x7;
    let t8 = t7 * c;
    let t9 = t6 - t8;
    let t10 = t4 + t9;
    let t11 = x4 - x13;
    let t12 = x12 + x5;
    let t13 = t11 - t12;
    let t14 = t13 * h;
    let t15 = x0 - x9;
    let t16 = t14 + t15;
    let t17 = t10 + t16;
    let t18 = t0 * s;
    let t19 = t2 * c;
    let t20 = t18 + t19;
    let t21 = t7 * s;
    let t22 = t5 * c;
    let t23 = t21 + t22;
    let t24 = t20 + t23;
    let t25 = t12 + t11;
    let t26 = t25 * h;
    let t27 = x1 + x8;
    let t28 = t26 + t27;
    let t29 = t24 + t28;
    let t30 = t16 - t10;
    let t31 = t28 - t24;
    let t32 = t15 - t14;
    let t33 = t20 - t23;
    let t34 = t32 - t33;
    let t35 = t27 - t26;
    let t36 = t4 - t9;
    let t37 = t35 + t36;
    let t38 = t32 + t33;
    let t39 = t35 - t36;
    let t40 = x11 + x2;
    let t41 = t40 * s;
    let t42 = x3 - x10;
    let t43 = t42 * c;
    let t44 = t41 - t43;
    let t45 = x15 + x6;
    let t46 = t45 * c;
    let t47 = x7 - x14;
    let t48 = t47 * s;
    let t49 = t46 - t48;
    let t50 = t44 - t49;
    let t51 = x9 + x0;
    let t52 = x5 - x12;
    let t53 = x13 + x4;
    let t54 = t52 + t53;
    let t55 = t54 * h;
    let t56 = t51 - t55;
    let t57 = t50 + t56;
    let t58 = t40 * c;
    let t59 = t42 * s;
    let t60 = t58 + t59;
    let t61 = t45 * s;
    let t62 = t47 * c;
    let t63 = t61 + t62;
    let t64 = t60 - t63;
    let t65 = t53 - t52;
    let t66 = t65 * h;
    let t67 = x1 - x8;
    let t68 = t66 + t67;
    let t69 = t64 + t68;
    let t70 = t56 - t50;
    let t71 = t68 - t64;
    let t72 = t55 + t51;
    let t73 = t63 + t60;
    let t74 = t72 - t73;
    let t75 = t49 + t44;
    let t76 = t67 - t66;
    let t77 = t75 + t76;
    let t78 = t73 + t72;
    let t79 = t76 - t75;
    [
        t17, t29, t30, t31, t34, t37, t38, t39, t57, t69, t70, t71, t74, t77, t78, t79,
    ]
}
