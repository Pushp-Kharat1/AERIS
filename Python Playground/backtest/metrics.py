from dataclasses import dataclass
import numpy as np

@dataclass(frozen=True)
class PerformanceMetrics:
    total_trades: int
    winning_trades: int
    losing_trades: int
    win_rate: float
    gross_pnl: float
    net_pnl: float
    total_fees: float
    total_tds: float
    profit_factor: float
    sharpe_ratio: float
    sortino_ratio: float
    max_drawdown_pct: float
    max_drawdown_abs: float
    calmar_ratio: float
    expectancy_per_trade: float

def compute_metrics(
    returns: np.ndarray,
    gross_pnl_series: np.ndarray,
    fees_series: np.ndarray,
    tds_series: np.ndarray,
    periods_per_year: float = 365.0 * 24.0 * 60.0
) -> PerformanceMetrics:
    if len(gross_pnl_series) == 0:
        return PerformanceMetrics(0, 0, 0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0)

    net_pnl_series = gross_pnl_series - fees_series - tds_series
    total_trades = len(net_pnl_series)
    wins = net_pnl_series[net_pnl_series > 0]
    losses = net_pnl_series[net_pnl_series < 0]

    winning_trades = int(len(wins))
    losing_trades = int(len(losses))
    win_rate = winning_trades / total_trades if total_trades > 0 else 0.0

    gross_pnl = float(np.sum(gross_pnl_series))
    net_pnl = float(np.sum(net_pnl_series))
    total_fees = float(np.sum(fees_series))
    total_tds = float(np.sum(tds_series))

    gross_win_sum = float(np.sum(wins)) if len(wins) > 0 else 0.0
    gross_loss_sum = float(np.abs(np.sum(losses))) if len(losses) > 0 else 0.0
    profit_factor = (gross_win_sum / gross_loss_sum) if gross_loss_sum > 0 else (np.inf if gross_win_sum > 0 else 0.0)

    mean_ret = float(np.mean(returns)) if len(returns) > 0 else 0.0
    std_ret = float(np.std(returns, ddof=1)) if len(returns) > 1 else 0.0
    sharpe_ratio = (mean_ret / std_ret * np.sqrt(periods_per_year)) if std_ret > 0 else 0.0

    downside_returns = returns[returns < 0]
    downside_std = float(np.std(downside_returns, ddof=1)) if len(downside_returns) > 1 else 0.0
    sortino_ratio = (mean_ret / downside_std * np.sqrt(periods_per_year)) if downside_std > 0 else 0.0

    cumulative = np.cumsum(net_pnl_series)
    running_max = np.maximum.accumulate(cumulative)
    drawdowns = running_max - cumulative
    max_drawdown_abs = float(np.max(drawdowns)) if len(drawdowns) > 0 else 0.0

    cum_with_base = running_max + 1e-9
    drawdown_pcts = drawdowns / cum_with_base
    max_drawdown_pct = float(np.max(drawdown_pcts)) if len(drawdown_pcts) > 0 else 0.0

    calmar_ratio = (net_pnl / max_drawdown_abs) if max_drawdown_abs > 0 else 0.0
    expectancy_per_trade = net_pnl / total_trades if total_trades > 0 else 0.0

    return PerformanceMetrics(
        total_trades=total_trades,
        winning_trades=winning_trades,
        losing_trades=losing_trades,
        win_rate=win_rate,
        gross_pnl=gross_pnl,
        net_pnl=net_pnl,
        total_fees=total_fees,
        total_tds=total_tds,
        profit_factor=profit_factor,
        sharpe_ratio=sharpe_ratio,
        sortino_ratio=sortino_ratio,
        max_drawdown_pct=max_drawdown_pct,
        max_drawdown_abs=max_drawdown_abs,
        calmar_ratio=calmar_ratio,
        expectancy_per_trade=expectancy_per_trade
    )

