/**
 * @file WebSocket.cpp
 *
 * This module contains the implementation of the WebSockets::WebSocket class.
 *
 * © 2018 by Richard Walters
 */

#include <Base64/Base64.hpp>
#include <Sha1/Sha1.hpp>
#include <stdint.h>
#include <SystemAbstractions/CryptoRandom.hpp>
#include <WebSockets/WebSocket.hpp>
#include <vector>

namespace {

    /**
     * This is the version of the WebSocket protocol that this class supports.
     */
    const std::string CURRENTLY_SUPPORTED_WEBSOCKET_VERSION = "13";

    /**
     * This is the required length of the Base64 decoding of the
     * "Sec-WebSocket-Key" header in HTTP requests that initiate a WebSocket
     * opening handshake.
     */
    constexpr size_t REQUIRED_WEBSOCKET_KEY_LENGTH = 16;

    /**
     * This is the string added to the "Sec-WebSocket-Key" before computing
     * the SHA-1 hash and Base64 encoding the result to form the
     * corresponding "Sec-WebSocket-Accept" value.
     */
    const std::string WEBSOCKET_KEY_SALT = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

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
     * This is the opcode for a continuation frame.
     */
    constexpr uint8_t OPCODE_CONTINUATION = 0x00;

    /**
     * This is the opcode for a text frame.
     */
    constexpr uint8_t OPCODE_TEXT = 0x01;

    /**
     * This is the opcode for a binary frame.
     */
    constexpr uint8_t OPCODE_BINARY = 0x02;

    /**
     * This is the opcode for a close frame.
     */
    constexpr uint8_t OPCODE_CLOSE = 0x08;

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

    /**
     * This is used to track what kind of message is being
     * sent or received in fragments.
     */
    enum class FragmentedMessageType {
        /**
         * This means we're not sending/receiving any message.
         */
        None,

        /**
         * This means we're sending/receiving a text message.
         */
        Text,

        /**
         * This means we're sending/receiving a binary message.
         */
        Binary,
    };

    /**
     * This function takes a string and swaps all upper-case characters
     * with their lower-case equivalents, returning the result.
     *
     * @param[in] inString
     *     This is the string to be normalized.
     *
     * @return
     *     The normalized string is returned.  All upper-case characters
     *     are replaced with their lower-case equivalents.
     */
    std::string NormalizeCaseInsensitiveString(const std::string& inString) {
        std::string outString;
        for (char c: inString) {
            outString.push_back(tolower(c));
        }
        return outString;
    }

    /**
     * This function computes the value of the "Sec-WebSocket-Accept"
     * HTTP response header that matches the given value of the
     * "Sec-WebSocket-Key" HTTP request header.
     *
     * @param[in] key
     *     This is the key value for which to compute the matching answer.
     *
     * @return
     *     The answer computed from the given key is returned.
     */
    std::string ComputeKeyAnswer(const std::string& key) {
        return Base64::Base64Encode(
            Sha1::Sha1(key + WEBSOCKET_KEY_SALT)
        );
    }

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
         * This is the Base64 encoded randomly-generated data set for
         * the Sec-WebSocket-Key header sent in the HTTP request
         * of the opening handshake, when opening as a client.
         */
        std::string key;

        /**
         * This flag indicates whether or not the WebSocket has sent
         * a close frame, and is waiting for a one to be received
         * back before closing the WebSocket.
         */
        bool closeSent = false;

        /**
         * This flag indicates whether or not the WebSocket has received
         * a close frame, and is waiting for the user to finish up
         * and signal a close, in order to close the WebSocket.
         */
        bool closeReceived = false;

        /**
         * This indicates what type of message the WebSocket is in the midst
         * of sending, if any.
         */
        FragmentedMessageType sending = FragmentedMessageType::None;

        /**
         * This indicates what type of message the WebSocket is in the midst
         * of receiving, if any.
         */
        FragmentedMessageType receiving = FragmentedMessageType::None;

        /**
         * This is the function to call whenever the WebSocket
         * has received a close frame or has been closed due to an error.
         */
        CloseReceivedDelegate closeDelegate;

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

        /**
         * This is the function to call whenever a text message
         * is received by the WebSocket.
         */
        MessageReceivedDelegate textDelegate;

        /**
         * This is the function to call whenever a binary message
         * is received by the WebSocket.
         */
        MessageReceivedDelegate binaryDelegate;

        /**
         * This is where we put data received before it's been
         * reassembled into frames.
         */
        std::vector< uint8_t > frameReassemblyBuffer;

        /**
         * This is where we put frames received before they've been
         * reassembled into messages.
         */
        std::string messageReassemblyBuffer;

        /**
         * This is used to generate masking keys that have strong entropy.
         */
        SystemAbstractions::CryptoRandom rng;

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
                uint8_t maskingKey[4];
                rng.Generate(maskingKey, sizeof(maskingKey));
                for (size_t i = 0; i < sizeof(maskingKey); ++i) {
                    frame.push_back(maskingKey[i]);
                }
                for (size_t i = 0; i < payload.length(); ++i) {
                    frame.push_back(payload[i] ^ maskingKey[i % 4]);
                }
            }
            connection->SendData(frame);
        }

        /**
         * This method is called whenever the WebSocket has reassembled
         * a complete frame received from the remote peer.
         *
         * @param[in] headerLength
         *     This is the size of the frame header, in octets.
         *
         * @param[in] payloadLength
         *     This is the size of the frame payload, in octets.
         */
        void ReceiveFrame(
            size_t headerLength,
            size_t payloadLength
        ) {
            const bool fin = ((frameReassemblyBuffer[0] & FIN) != 0);
            const uint8_t reservedBits = ((frameReassemblyBuffer[0] >> 4) & 0x07);
            if (reservedBits != 0) {
                // TODO: protocol violation -- reserved bits
                // must be zero unless some extension that uses
                // them has been enabled through the open handshake.
            }
            const uint8_t opcode = (frameReassemblyBuffer[0] & 0x0F);
            std::string data;
            if (role == Role::Server) {
                data.resize(payloadLength);
                for (size_t i = 0; i < payloadLength; ++i) {
                    data[i] = (
                        frameReassemblyBuffer[headerLength + i]
                        ^ frameReassemblyBuffer[headerLength - 4 + (i % 4)]
                    );
                }
            } else {
                (void)data.assign(
                    frameReassemblyBuffer.begin() + headerLength,
                    frameReassemblyBuffer.begin() + headerLength + payloadLength
                );
            }
            switch (opcode) {
                case OPCODE_CONTINUATION: {
                    messageReassemblyBuffer += data;
                    switch (receiving) {
                        case FragmentedMessageType::Text: {
                            if (fin) {
                                if (textDelegate != nullptr) {
                                    textDelegate(messageReassemblyBuffer);
                                }
                            }
                        } break;

                        case FragmentedMessageType::Binary: {
                            if (fin) {
                                if (binaryDelegate != nullptr) {
                                    binaryDelegate(messageReassemblyBuffer);
                                }
                            }
                        } break;

                        default: {
                            messageReassemblyBuffer.clear();
                            // TODO: unexpected continuation
                        } break;
                    }
                    if (fin) {
                        receiving = FragmentedMessageType::None;
                        messageReassemblyBuffer.clear();
                    }
                } break;

                case OPCODE_TEXT: {
                    if (receiving == FragmentedMessageType::None) {
                        if (fin) {
                            if (textDelegate != nullptr) {
                                textDelegate(data);
                            }
                        } else {
                            receiving = FragmentedMessageType::Text;
                            messageReassemblyBuffer = data;
                        }
                    } else {
                        // TODO: protocol violation -- start of next message
                        // before last was complete.
                    }
                } break;

                case OPCODE_BINARY: {
                    if (receiving == FragmentedMessageType::None) {
                        if (fin) {
                            if (binaryDelegate != nullptr) {
                                binaryDelegate(data);
                            }
                        } else {
                            receiving = FragmentedMessageType::Binary;
                            messageReassemblyBuffer = data;
                        }
                    } else {
                        // TODO: protocol violation -- start of next message
                        // before last was complete.
                    }
                } break;

                case OPCODE_CLOSE: {
                    unsigned int code = 1005;
                    std::string reason;
                    if (data.length() >= 2) {
                        code = (
                            (((unsigned int)data[0] << 8) & 0xFF00)
                            + ((unsigned int)data[1] & 0x00FF)
                        );
                        reason = data.substr(2);
                    }
                    const auto closeSentEarlier = closeSent;
                    closeReceived = true;
                    if (closeDelegate != nullptr) {
                        closeDelegate(code, reason);
                    }
                    if (closeSentEarlier) {
                        connection->Break(false);
                    }
                } break;

                case OPCODE_PING: {
                    if (pingDelegate != nullptr) {
                        pingDelegate(data);
                    }
                    SendFrame(true, OPCODE_PONG, data);
                } break;

                case OPCODE_PONG: {
                    if (pongDelegate != nullptr) {
                        pongDelegate(data);
                    }
                } break;

                default: {
                    // TODO: protocol violation -- unknown opcode.
                } break;
            }
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
            (void)frameReassemblyBuffer.insert(
                frameReassemblyBuffer.end(),
                data.begin(),
                data.end()
            );
            for(;;) {
                if (frameReassemblyBuffer.size() < 2) {
                    return;
                }
                const auto lengthFirstOctet = (frameReassemblyBuffer[1] & ~MASK);
                size_t headerLength, payloadLength;
                if (lengthFirstOctet == 0x7E) {
                    headerLength = 4;
                    if (frameReassemblyBuffer.size() < headerLength) {
                        return;
                    }
                    payloadLength = (
                        ((size_t)frameReassemblyBuffer[2] << 8)
                        + (size_t)frameReassemblyBuffer[3]
                    );
                } else if (lengthFirstOctet == 0x7F) {
                    headerLength = 10;
                    if (frameReassemblyBuffer.size() < headerLength) {
                        return;
                    }
                    payloadLength = (
                        ((size_t)frameReassemblyBuffer[2] << 56)
                        + ((size_t)frameReassemblyBuffer[3] << 48)
                        + ((size_t)frameReassemblyBuffer[4] << 40)
                        + ((size_t)frameReassemblyBuffer[5] << 32)
                        + ((size_t)frameReassemblyBuffer[6] << 24)
                        + ((size_t)frameReassemblyBuffer[7] << 16)
                        + ((size_t)frameReassemblyBuffer[8] << 8)
                        + (size_t)frameReassemblyBuffer[9]
                    );
                } else {
                    headerLength = 2;
                    payloadLength = (size_t)lengthFirstOctet;
                }
                if (role == Role::Server) {
                    headerLength += 4;
                }
                if (frameReassemblyBuffer.size() < headerLength + payloadLength) {
                    return;
                }
                ReceiveFrame(headerLength, payloadLength);
                (void)frameReassemblyBuffer.erase(
                    frameReassemblyBuffer.begin(),
                    frameReassemblyBuffer.begin() + headerLength + payloadLength
                );
            }
        }
    };

    WebSocket::~WebSocket() = default;

    WebSocket::WebSocket()
        : impl_(new Impl)
    {
    }

    void WebSocket::StartOpenAsClient(
        Http::Request& request
    ) {
        request.headers.SetHeader("Sec-WebSocket-Version", CURRENTLY_SUPPORTED_WEBSOCKET_VERSION);
        char nonce[16];
        impl_->rng.Generate(nonce, sizeof(nonce));
        impl_->key = Base64::Base64Encode(
            std::string(nonce, sizeof(nonce))
        );
        request.headers.SetHeader("Sec-WebSocket-Key", impl_->key);
        request.headers.SetHeader("Upgrade", "websocket");
        auto connectionTokens = request.headers.GetHeaderMultiValue("Connection");
        connectionTokens.push_back("upgrade");
        request.headers.SetHeader("Connection", connectionTokens, true);
    }

    bool WebSocket::FinishOpenAsClient(
        std::shared_ptr< Http::Connection > connection,
        const Http::Response& response
    ) {
        if (response.statusCode != 101) {
            return false;
        }
        bool foundUpgradeToken = false;
        for (const auto token: response.headers.GetHeaderTokens("Connection")) {
            if (token == "upgrade") {
                foundUpgradeToken = true;
                break;
            }
        }
        if (!foundUpgradeToken) {
            return false;
        }
        if (NormalizeCaseInsensitiveString(response.headers.GetHeaderValue("Upgrade")) != "websocket") {
            return false;
        }
        if (response.headers.GetHeaderValue("Sec-WebSocket-Accept") != ComputeKeyAnswer(impl_->key)) {
            return false;
        }
        Open(connection, Role::Client);
        return true;
    }

    bool WebSocket::OpenAsServer(
        std::shared_ptr< Http::Connection > connection,
        const Http::Request& request,
        Http::Response& response
    ) {
        if (request.headers.GetHeaderValue("Sec-WebSocket-Version") != CURRENTLY_SUPPORTED_WEBSOCKET_VERSION) {
            return false;
        }
        bool foundUpgradeToken = false;
        for (const auto token: request.headers.GetHeaderTokens("Connection")) {
            if (token == "upgrade") {
                foundUpgradeToken = true;
                break;
            }
        }
        if (!foundUpgradeToken) {
            return false;
        }
        if (NormalizeCaseInsensitiveString(request.headers.GetHeaderValue("Upgrade")) != "websocket") {
            return false;
        }
        impl_->key = request.headers.GetHeaderValue("Sec-WebSocket-Key");
        if (Base64::Base64Decode(impl_->key).length() != REQUIRED_WEBSOCKET_KEY_LENGTH) {
            return false;
        }
        auto connectionTokens = response.headers.GetHeaderMultiValue("Connection");
        connectionTokens.push_back("upgrade");
        response.statusCode = 101;
        response.reasonPhrase = "Switching Protocols";
        response.headers.SetHeader("Connection", connectionTokens, true);
        response.headers.SetHeader("Upgrade", "websocket");
        response.headers.SetHeader("Sec-WebSocket-Accept", ComputeKeyAnswer(impl_->key));
        Open(connection, Role::Server);
        return true;
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

    void WebSocket::Close(
        unsigned int code,
        const std::string reason
    ) {
        if (impl_->closeSent) {
            return;
        }
        impl_->closeSent = true;
        if (code == 1006) {
            impl_->connection->Break(false);
        } else {
            std::string data;
            if (code != 1005) {
                data.push_back((uint8_t)(code >> 8));
                data.push_back((uint8_t)(code & 0xFF));
                data += reason;
            }
            impl_->SendFrame(true, OPCODE_CLOSE, data);
            if (impl_->closeReceived) {
                impl_->connection->Break(true);
            }
        }
    }

    void WebSocket::Ping(const std::string& data) {
        if (impl_->closeSent) {
            return;
        }
        if (data.length() > MAX_CONTROL_FRAME_DATA_LENGTH) {
            return;
        }
        impl_->SendFrame(true, OPCODE_PING, data);
    }

    void WebSocket::Pong(const std::string& data) {
        if (impl_->closeSent) {
            return;
        }
        if (data.length() > MAX_CONTROL_FRAME_DATA_LENGTH) {
            return;
        }
        impl_->SendFrame(true, OPCODE_PONG, data);
    }

    void WebSocket::SendText(
        const std::string& data,
        bool lastFragment
    ) {
        if (impl_->closeSent) {
            return;
        }
        if (impl_->sending == FragmentedMessageType::Binary) {
            return;
        }
        const auto opcode = (
            (impl_->sending == FragmentedMessageType::Text)
            ? OPCODE_CONTINUATION
            : OPCODE_TEXT
        );
        impl_->SendFrame(lastFragment, opcode, data);
        impl_->sending = (
            lastFragment
            ? FragmentedMessageType::None
            : FragmentedMessageType::Text
        );
    }

    void WebSocket::SendBinary(
        const std::string& data,
        bool lastFragment
    ) {
        if (impl_->closeSent) {
            return;
        }
        if (impl_->sending == FragmentedMessageType::Text) {
            return;
        }
        const auto opcode = (
            (impl_->sending == FragmentedMessageType::Binary)
            ? OPCODE_CONTINUATION
            : OPCODE_BINARY
        );
        impl_->SendFrame(lastFragment, opcode, data);
        impl_->sending = (
            lastFragment
            ? FragmentedMessageType::None
            : FragmentedMessageType::Binary
        );
    }

    void WebSocket::SetCloseDelegate(CloseReceivedDelegate closeDelegate) {
        impl_->closeDelegate = closeDelegate;
    }

    void WebSocket::SetPingDelegate(MessageReceivedDelegate pingDelegate) {
        impl_->pingDelegate = pingDelegate;
    }

    void WebSocket::SetPongDelegate(MessageReceivedDelegate pongDelegate) {
        impl_->pongDelegate = pongDelegate;
    }

    void WebSocket::SetTextDelegate(MessageReceivedDelegate textDelegate) {
        impl_->textDelegate = textDelegate;
    }

    void WebSocket::SetBinaryDelegate(MessageReceivedDelegate binaryDelegate) {
        impl_->binaryDelegate = binaryDelegate;
    }

}
