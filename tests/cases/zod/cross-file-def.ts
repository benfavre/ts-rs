// @expect: no-errors
// @hover schema: ZodString
//
// Smoke test: importing `z` from `zod` and using it must hover with
// the zod-specific subclass, and the import target must be reachable.
// Note we don't @def-assert into node_modules/zod (the path is
// version-specific) but we do check that goto-def stays in this file
// for the local binding.

import { z } from "zod";

const schema = z.string();

// Cross-reference: another local declaration should still go-to-def
// to itself, not get confused by the zod import.
// @def schema -> cross-file-def.ts:12:7

export { schema };
