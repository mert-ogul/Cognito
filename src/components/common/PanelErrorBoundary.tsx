import { Button } from "@/components/ui/button";
import { Component, type ErrorInfo, type ReactNode } from "react";

type Props = {
  children: ReactNode;
};

type State = {
  error: Error | null;
};

export class PanelErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("panel error", error, info.componentStack);
  }

  render() {
    if (this.state.error) {
      return (
        <div className="flex h-full min-h-0 flex-col gap-2 p-3">
          <p className="font-medium text-text">This panel hit an error</p>
          <p className="text-danger" role="alert">
            {this.state.error.message}
          </p>
          <Button type="button" variant="secondary" onClick={() => this.setState({ error: null })}>
            Try again
          </Button>
        </div>
      );
    }
    return this.props.children;
  }
}
