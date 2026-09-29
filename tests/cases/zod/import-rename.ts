// @expect: no-errors
// @hover renamed: module
// @hover sch: ZodString
//
// `import { z as zod } from "zod"` renames z → zod in the local scope.
// Hover on the renamed binding should still show the zod namespace
// shape, and member access continues to resolve through the renamed
// alias.

import { z as zod } from "zod";

const renamed = zod;
const sch = zod.string();

export { renamed, sch };
