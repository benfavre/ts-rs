// @module: commonjs
// @target: es2015
// @filename: enum.ts
export var Enum;
(function (Enum) {
    Enum[Enum["A"] = 0] = "A";
    Enum[Enum["B"] = 1] = "B";
})(Enum || (Enum = {}));
// @filename: alias.ts
import { Enum } from "./enum";
