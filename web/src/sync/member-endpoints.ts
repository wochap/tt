// The worker's endpoint list for the signed-in session: seeded from the
// stored session (or the sign-in server), refreshed from `/api/peers` of the
// member just connected to, and handed to the tabs to persist with the
// session. Signing out drops it.

import { markIncompatible, markOk, MemberRotation, refreshEndpoints, seedEndpoints } from "./endpoints.ts";
import type { Auth, Endpoint } from "./protocol.ts";
import { fetchPeers } from "./server-api.ts";

export class MemberEndpoints {
  readonly #persist: (token: string, endpoints: Endpoint[]) => void;
  #list: Endpoint[] = [];
  #auth: Auth | null = null;
  #rotation: MemberRotation | undefined;

  constructor(persist: (token: string, endpoints: Endpoint[]) => void) {
    this.#persist = persist;
  }

  get list(): Endpoint[] {
    return this.#list;
  }

  /** The member of the current or last connection attempt, as the list knows it now. */
  get current(): Endpoint | null {
    const current = this.#rotation?.current;
    if (!current) return null;
    return this.#list.find((endpoint) => endpoint.public_url === current.public_url) ?? current;
  }

  /** Starts over for a new session (or none); `onChange` hears about compatibility changes. */
  reset(auth: Auth | null, onChange: () => void): MemberRotation | undefined {
    this.#auth = auth;
    this.#list = auth ? seedEndpoints(auth) : [];
    this.#rotation = auth
      ? new MemberRotation({
          endpoints: () => this.#list,
          token: () => auth.token,
          onCompatibility: (endpoint, incompatible) => {
            this.#list = markIncompatible(this.#list, endpoint.public_url, incompatible);
            onChange();
          },
        })
      : undefined;
    return this.#rotation;
  }

  /** The socket of `auth`'s session connected: records the success and refreshes the list from that member. */
  async connected(auth: Auth): Promise<void> {
    const rotation = this.#rotation;
    const member = rotation?.current;
    if (!rotation || !member || this.#auth !== auth) return;
    rotation.connected();
    this.#list = markOk(this.#list, member.public_url, Date.now());
    const peers = await fetchPeers({ server: member.public_url, token: auth.token });
    if (this.#auth !== auth) return;
    if (peers) this.#list = refreshEndpoints(this.#list, peers, member, auth.server);
    this.#persist(auth.token, this.#list);
  }
}
