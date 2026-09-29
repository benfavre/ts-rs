// @expect: no-errors
//
// Building object schemas. The shapes here are exactly what TRPC routers
// in this repo declare for `input(...)` validation.

import { z } from "zod";

const CreateProductSchema = z.object({
  name: z.string().min(1),
  price: z.number().positive(),
  categoryId: z.string().uuid(),
});

const ListProductsSchema = z.object({
  page: z.number().min(1).default(1),
  pageSize: z.number().min(1).max(100).default(20),
  search: z.string().optional(),
  categoryId: z.string().uuid().optional(),
});

// Just constructing schemas should never error.
export { CreateProductSchema, ListProductsSchema };
