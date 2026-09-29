// @expect: no-errors
//
// The repo's canonical TRPC pattern: declare a Zod schema, infer the input
// type, write a handler typed against it. This fixture stops short of
// actually calling TRPC (avoids dragging the whole @trpc/server type tree)
// but exercises the same z.infer<typeof ...> contract.

import { z } from "zod";

const createOrderSchema = z.object({
  customerId: z.string().uuid(),
  items: z.array(z.object({
    productId: z.string().uuid(),
    quantity: z.number().int().positive(),
  })),
  notes: z.string().optional(),
});

type CreateOrderInput = z.infer<typeof createOrderSchema>;

function handler(input: CreateOrderInput) {
  // `input.items` should be an array of { productId; quantity }
  for (const item of input.items) {
    void item.productId;
    void item.quantity;
  }
  return input.notes ?? "";
}

export { createOrderSchema, handler };
