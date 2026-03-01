import { Component } from 'react';

export default class ErrorBoundary extends Component {
  constructor(props) {
    super(props);
    this.state = { error: null };
  }

  static getDerivedStateFromError(error) {
    return { error };
  }

  componentDidCatch(error, info) {
    console.error('ErrorBoundary caught:', error, info.componentStack);
  }

  render() {
    if (this.state.error) {
      return (
        <div className="flex items-center justify-center h-full p-8">
          <div className="bg-summit-red/8 border border-summit-red/15 rounded-xl p-6 max-w-md text-center">
            <div className="text-[12px] font-bold text-summit-red mb-2">Something went wrong</div>
            <div className="text-[10px] text-white/40 font-mono mb-4 break-all">
              {this.state.error.message}
            </div>
            <button
              onClick={() => this.setState({ error: null })}
              className="px-4 py-1.5 rounded-lg text-[10px] font-bold bg-white/[0.04] border border-white/10 text-white/50 hover:bg-white/[0.08] cursor-pointer transition-all"
            >
              Try Again
            </button>
          </div>
        </div>
      );
    }

    return this.props.children;
  }
}
