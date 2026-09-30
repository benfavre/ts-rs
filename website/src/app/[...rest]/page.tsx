// Catch-all: any path no other route matches answers 404 from its loader, so
// the framework renders not-found.tsx inside the layout instead of a bare
// 404 body. force-dynamic keeps these out of the ISR cache.
export const dynamic = "force-dynamic";

export async function loader(): Promise<Response> {
  return new Response(null, { status: 404 });
}

export default function CatchAll(): any {
  return null as any;
}
