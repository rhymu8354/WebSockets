/**
 * @file WebSocket.cpp
 *
 * This module contains the implementation of the WebSockets::WebSocket class.
 *
 * © 2018 by Richard Walters
 */

#include <stdint.h>
#include <WebSockets/WebSocket.hpp>
#include <vector>

namespace {

    /**
     * This is the bit to set in the first octet of a WebSocket frame
     * to indicate that the frame is the final one in a message.
     */
    constexpr uint8_t FIN = 0x80;

    /**
     * This is the bit to set in the second octet of a WebSocket frame
     * to indicate that the payload of the frame is masked, and that
     * a masking key is included.
     */
    constexpr uint8_t MASK = 0x80;

    /**
     * This is the opcode for a ping frame.
     */
    constexpr uint8_t OPCODE_PING = 0x09;

    /**
     * This is the opcode for a pong frame.
     */
    constexpr uint8_t OPCODE_PONG = 0x0A;

    /**
     * This is the maximum length of a control frame payload.
     */
    constexpr size_t MAX_CONTROL_FRAME_DATA_LENGTH = 125;

}

namespace WebSockets {

    /**
     * This contains the private properties of a WebSocket instance.
     */
    struct WebSocket::Impl {
        // Properties

        /**
         * This is the connection to use to send and receive frames.
         */
        std::shared_ptr< Http::Connection > connection;

        /**
         * This is the role to play in the connection.
         */
        Role role;

        /**
         * This is the function to call whenever a ping message
         * is received by the WebSocket.
         */
        MessageReceivedDelegate pingDelegate;

        /**
         * This is the function to call whenever a pong message
         * is received by the WebSocket.
         */
        MessageReceivedDelegate pongDelegate;

        // Methods

        /**
         * This method constructs and sends a frame from the WebSocket.
         *
         * @param[in] fin
         *     This indicates whether or not to set the FIN bit in the frame.
         *
         * @param[in] opcode
         *     This is the opcode to set in the frame.
         *
         * @param[in] payload
         *     This is the payload to include in the frame.
         */
        void SendFrame(
            bool fin,
            uint8_t opcode,
            const std::string& payload
        ) {
            std::vector< uint8_t > frame;
            frame.push_back(
                (fin ? FIN : 0)
                + opcode
            );
            const uint8_t mask = ((role == Role::Client) ? MASK : 0);
            if (payload.length() < 126) {
                frame.push_back((uint8_t)payload.length() + mask);
            } else if (payload.length() < 65536) {
                frame.push_back(0x7E + mask);
                frame.push_back((uint8_t)(payload.length() >> 8));
                frame.push_back((uint8_t)(payload.length() & 0xFF));
            } else {
                frame.push_back(0x7F + mask);
                frame.push_back((uint8_t)(payload.length() >> 56));
                frame.push_back((uint8_t)((payload.length() >> 48) & 0xFF));
                frame.push_back((uint8_t)((payload.length() >> 40) & 0xFF));
                frame.push_back((uint8_t)((payload.length() >> 32) & 0xFF));
                frame.push_back((uint8_t)((payload.length() >> 24) & 0xFF));
                frame.push_back((uint8_t)((payload.length() >> 16) & 0xFF));
                frame.push_back((uint8_t)((payload.length() >> 8) & 0xFF));
                frame.push_back((uint8_t)(payload.length() & 0xFF));
            }
            if (mask == 0) {
                (void)frame.insert(
                    frame.end(),
                    payload.begin(),
                    payload.end()
                );
            } else {
                // TODO: need to pick one at random from a strong
                // source of entropy.
                uint8_t maskingKey[4] = {0xDE, 0xAD, 0xBE, 0xEF};
                for (size_t i = 0; i < payload.length(); ++i) {
                    frame.push_back(payload[i] ^ maskingKey[i % 4]);
                }
            }
            connection->SendData(frame);
        }

        /**
         * This method is called whenever the WebSocket receives data from
         * the remote peer.
         *
         * @param[in] data
         *     This is the data received from the remote peer.
         */
        void ReceiveData(
            const std::vector< uint8_t >& data
        ) {
        }
    };

    WebSocket::~WebSocket() = default;

    WebSocket::WebSocket()
        : impl_(new Impl)
    {
    }

    void WebSocket::Open(
        std::shared_ptr< Http::Connection > connection,
        Role role
    ) {
        impl_->connection = connection;
        impl_->role = role;
        impl_->connection->SetDataReceivedDelegate(
            [this](
                const std::vector< uint8_t >& data
            ){
                impl_->ReceiveData(data);
            }
        );
    }

    void WebSocket::Ping(const std::string& data) {
        if (data.length() > MAX_CONTROL_FRAME_DATA_LENGTH) {
            return;
        }
        impl_->SendFrame(true, OPCODE_PING, data);
    }

    void WebSocket::Pong(const std::string& data) {
        if (data.length() > MAX_CONTROL_FRAME_DATA_LENGTH) {
            return;
        }
        impl_->SendFrame(true, OPCODE_PONG, data);
    }

    void WebSocket::SetPingDelegate(MessageReceivedDelegate pingDelegate) {
        impl_->pingDelegate = pingDelegate;
    }

    void WebSocket::SetPongDelegate(MessageReceivedDelegate pongDelegate) {
        impl_->pongDelegate = pongDelegate;
    }

}
