// The shape every catalogue has (L-1): English's keys and function arguments, with
// strings widened, so a missing key or a wrong argument list fails the type check.

import type { en } from './en'

type Widen<T> = T extends string
  ? string
  : T extends (...args: infer A) => string
    ? (...args: A) => string
    : { readonly [K in keyof T]: Widen<T[K]> }

export type Messages = Widen<typeof en>

/** The UI languages (L-1). `en-XA` is the pseudo-locale, in development builds only. */
export type Locale = 'en' | 'zh-Hans' | 'zh-Hant' | 'ja' | 'es' | 'en-XA'
