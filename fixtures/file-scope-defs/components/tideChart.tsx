import { Component } from "@deck/parts";
import { withLabels } from "@deck/labels";

interface TideChartProps {
  values: number[];
}

interface TideChartState {
  spread: number;
}

class TideChart extends Component<TideChartProps, TideChartState> {
  render() {
    return <section>{this.props.values.length}</section>;
  }
}

export default withLabels(TideChart);
