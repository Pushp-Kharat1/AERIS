import numpy as np

class OrderFlowImbalanceStrategy:
    def __init__(self, depth_levels: int = 5, ofi_threshold: float = 1.5):
        self.depth_levels = depth_levels
        self.ofi_threshold = ofi_threshold

    def compute_level_ofi(
        self,
        prev_bid_px: float,
        curr_bid_px: float,
        prev_bid_sz: float,
        curr_bid_sz: float,
        prev_ask_px: float,
        curr_ask_px: float,
        prev_ask_sz: float,
        curr_ask_sz: float
    ) -> float:
        if curr_bid_px > prev_bid_px:
            delta_bid = curr_bid_sz
        elif curr_bid_px == prev_bid_px:
            delta_bid = curr_bid_sz - prev_bid_sz
        else:
            delta_bid = -prev_bid_sz

        if curr_ask_px < prev_ask_px:
            delta_ask = curr_ask_sz
        elif curr_ask_px == prev_ask_px:
            delta_ask = curr_ask_sz - prev_ask_sz
        else:
            delta_ask = -prev_ask_sz

        return delta_bid - delta_ask

    def compute_multilevel_ofi(
        self,
        prev_bids: np.ndarray,
        curr_bids: np.ndarray,
        prev_asks: np.ndarray,
        curr_asks: np.ndarray
    ) -> float:
        total_ofi = 0.0
        levels = min(self.depth_levels, len(prev_bids), len(curr_bids), len(prev_asks), len(curr_asks))

        for k in range(levels):
            decay = 1.0 / (k + 1.0)
            level_ofi = self.compute_level_ofi(
                prev_bids[k, 0], curr_bids[k, 0], prev_bids[k, 1], curr_bids[k, 1],
                prev_asks[k, 0], curr_asks[k, 0], prev_asks[k, 1], curr_asks[k, 1]
            )
            total_ofi += decay * level_ofi

        return total_ofi

    def generate_signal(self, ofi_value: float, std_ofi: float) -> int:
        if std_ofi <= 0:
            return 0
        z_score = ofi_value / std_ofi
        if z_score > self.ofi_threshold:
            return 1
        elif z_score < -self.ofi_threshold:
            return -1
        return 0

