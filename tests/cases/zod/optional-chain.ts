// @expect: no-errors
// @hover deepName: string | undefined
// @hover safeLen: number | undefined
// @hover firstId: number | undefined
//
// Optional chaining (`?.`) — the result type must always include
// `undefined`. Without it, callers do `obj?.x.y` and the checker
// silently widens to `string`, missing real null-dereference bugs.

interface User {
  name?: string;
  profile?: { avatar?: { url: string } };
}

const user: User = { name: "alice" };

const deepName = user.profile?.avatar?.url;
const safeLen = user.name?.length;

const users: User[] = [user];
const firstId = users[0]?.profile?.avatar?.url?.length;

export { deepName, safeLen, firstId };
