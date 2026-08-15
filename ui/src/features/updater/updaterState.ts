export type UpdateMetadata = {
  version: string;
  date: string | null;
  body: string | null;
};

export type UpdateOperation = "check" | "download" | "install";

export type UpdateState =
  | { status: "idle" }
  | { status: "checking" }
  | { status: "upToDate"; checkedAt: string }
  | {
      status: "downloading";
      update: UpdateMetadata;
      downloaded: number;
      contentLength: number | null;
    }
  | { status: "ready"; update: UpdateMetadata; dismissed: boolean }
  | { status: "installing"; update: UpdateMetadata }
  | { status: "error"; operation: UpdateOperation; message: string; update: UpdateMetadata | null };

export type UpdateEvent =
  | { type: "CHECK_STARTED" }
  | { type: "NO_UPDATE"; checkedAt: string }
  | { type: "DOWNLOAD_STARTED"; update: UpdateMetadata; contentLength: number | null }
  | { type: "DOWNLOAD_PROGRESS"; chunkLength: number }
  | { type: "DOWNLOAD_FINISHED" }
  | { type: "DISMISS" }
  | { type: "INSTALL_STARTED" }
  | { type: "FAILED"; operation: UpdateOperation; message: string; update?: UpdateMetadata | null };

export const INITIAL_UPDATE_STATE: UpdateState = { status: "idle" };

function stateUpdate(state: UpdateState): UpdateMetadata | null {
  return "update" in state ? state.update : null;
}

export function updateReducer(state: UpdateState, event: UpdateEvent): UpdateState {
  switch (event.type) {
    case "CHECK_STARTED":
      return { status: "checking" };
    case "NO_UPDATE":
      return { status: "upToDate", checkedAt: event.checkedAt };
    case "DOWNLOAD_STARTED":
      return {
        status: "downloading",
        update: event.update,
        downloaded: 0,
        contentLength: event.contentLength,
      };
    case "DOWNLOAD_PROGRESS":
      if (state.status !== "downloading") {
        return state;
      }
      return {
        ...state,
        downloaded:
          state.contentLength === null
            ? state.downloaded + event.chunkLength
            : Math.min(state.downloaded + event.chunkLength, state.contentLength),
      };
    case "DOWNLOAD_FINISHED":
      return state.status === "downloading"
        ? { status: "ready", update: state.update, dismissed: false }
        : state;
    case "DISMISS":
      return state.status === "ready" ? { ...state, dismissed: true } : state;
    case "INSTALL_STARTED":
      return state.status === "ready" ? { status: "installing", update: state.update } : state;
    case "FAILED":
      return {
        status: "error",
        operation: event.operation,
        message: event.message,
        update: event.update === undefined ? stateUpdate(state) : event.update,
      };
  }
}
