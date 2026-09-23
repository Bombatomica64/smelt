type Next = () => Promise<void>;
type Handler = (c: string, next: Next) => Promise<void>;
type Options = { prefix?: string };
export const requestId = ({ prefix = "id-" }: Options = {}): Handler => {
  return async function requestId(c, next) {
    console.log(prefix + c);
    await next();
  };
};
