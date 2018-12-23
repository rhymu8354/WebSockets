#ifndef WEB_SOCKETS_MAKE_CONNECTION_HPP
#define WEB_SOCKETS_MAKE_CONNECTION_HPP

/**
 * @file MakeConnection.hpp
 *
 * This module declares the WebSockets::MakeConnection function.
 *
 * © 2018 by Richard Walters
 */

#include <functional>
#include <future>
#include <Http/IClient.hpp>
#include <memory>
#include <stdint.h>
#include <string>
#include <SystemAbstractions/DiagnosticsSender.hpp>
#include <WebSockets/WebSocket.hpp>

namespace WebSockets {

    /**
     * This is used to return values from the MakeConnection function.
     */
    struct MakeConnectionResults {
        /**
         * This is a mechanism to access the result of the connection attempt.
         * If the connection is successful, it will yield a WebSocket object
         * reference.  Otherwise, it will yield nullptr, indicating that the
         * connection could not be made.
         */
        std::future< std::shared_ptr< WebSocket > > connectionFuture;

        /**
         * This is a function which can be called to abort the connection
         * attempt early.
         */
        std::function< void() > abortConnection;
    };

    /**
     * This method is called to asynchronously attempt to connect to a web
     * server and upgrade the connection a WebSocket.
     *
     * @param[in] http
     *     This is the web client object to use to make the connection.
     *
     * @param[in] host
     *     This is the host name or IP address of the server to which to
     *     connect.
     *
     * @param[in] port
     *     This is the port number of the server to which to connect.
     *
     * @param[in] diagnosticsSender
     *     This is the object to use to publish any diagnostic messages.
     *
     * @return
     *     A structure is returned containing information and tools to
     *     use in coordinating with the asynchronous connection operation.
     */
    MakeConnectionResults MakeConnection(
        std::shared_ptr< Http::IClient > http,
        const std::string& host,
        uint16_t port,
        std::shared_ptr< SystemAbstractions::DiagnosticsSender > diagnosticsSender
    );

}

#endif /* WEB_SOCKETS_MAKE_CONNECTION_HPP */
