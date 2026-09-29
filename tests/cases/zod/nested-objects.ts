// @expect: no-errors
//
// Deeply nested object schemas — common shape for address/contact fields.

import { z } from "zod";

const AddressSchema = z.object({
  street: z.string(),
  city: z.string(),
  postalCode: z.string().regex(/^\d{5}$/),
  country: z.string().length(2),
});

const ContactSchema = z.object({
  email: z.string().email(),
  phone: z.string().optional(),
  address: AddressSchema,
});

const PersonSchema = z.object({
  name: z.string(),
  age: z.number().int().nonnegative(),
  contact: ContactSchema,
  emergencyContact: ContactSchema.optional(),
});

export { AddressSchema, ContactSchema, PersonSchema };
