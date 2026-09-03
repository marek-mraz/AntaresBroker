*** Settings ***
Documentation       Verify the policy seam end to end, against a broker built
...                 with the reference engine of `examples/plugin-example`.
...
...                 ANTARES DECISION, NOT A CIM 009 REQUIREMENT. CIM 009 gives
...                 the broker no authorization model: clause 4.13 leaves
...                 security to the deployment and Table 6.3.2-1 names no
...                 access-denied error. So a refusal here answers this
...                 broker's own `urn:antares:error:AccessDenied` rather than
...                 an `https://uri.etsi.org/` type it would be inventing, and
...                 a narrowed answer is marked with an `Antares-` header.
...
...                 The reason the seam exists is that three narrowings cannot
...                 be done by the gateway in front: the query the store runs,
...                 one subscription's notification, and a federated result
...                 before it is rendered (ADR-0020). Everything else stays in
...                 the gateway, and the broker ships exactly one engine —
...                 allow-all — with every other one an addon outside
...                 `crates/`, which is what this file exercises.
...
...                 The rules the run is started with:
...                 tenant `acme` denies the Entity type `PolicySecret` and
...                 omits the Attribute `price`; tenant `beta` conjoins
...                 `speed<100` into every query; every other tenant is
...                 unrestricted.

Library             Collections
Library             RequestsLibrary
Library             HttpCtrl.Server
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource
Variables           ${EXECDIR}/resources/variables.py

Suite Setup         Mint The Suite Ids


*** Variables ***
${core}                     https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld
${restricted_header}        Antares-Results-Restricted
${subject_header}           X-Subject


*** Test Cases ***
A Denied Entity Type Is Refused With 403
    [Documentation]    The engine refuses `PolicySecret` for this tenant, and
    ...    the refusal is a 403 carrying the broker's own error type — not a
    ...    404 that would hide a policy decision behind a spec error, and not
    ...    an ETSI error type for a condition CIM 009 does not name.
    [Tags]    antares-specific    policy    adr-0020
    ${payload}=    Evaluate
    ...    {"id": "urn:ngsi-ld:PolicySecret:" + $suffix, "type": "PolicySecret", "@context": $core}
    ${response}=    Create As    acme    ${payload}
    Check Response Status Code    403    ${response.status_code}
    Should Be Equal As Strings    ${response.json()}[type]    urn:antares:error:AccessDenied

An Allowed Entity Type In The Same Tenant Still Lands
    [Documentation]    The refusal has to be the engine's rule and not the
    ...    seam refusing everything: the same tenant, one type further, is a
    ...    201. A test asserting only the 403 would pass against a broker
    ...    that had stopped serving.
    [Tags]    antares-specific    policy    adr-0020
    ${payload}=    Evaluate
    ...    {"id": $priced_id, "type": "PolicyVehicle", "speed": {"type": "Property", "value": 10}, "price": {"type": "Property", "value": 42}, "@context": $core}
    ${response}=    Create As    acme    ${payload}
    Check Response Status Code    201    ${response.status_code}

A Filtered Query Hides The Entity It Excludes
    [Documentation]    The engine conjoins `speed<100` into this tenant's
    ...    queries, so the fast Entity is not in the answer at all — the
    ...    narrowing runs in the store, which is why it cannot be done by the
    ...    gateway in front. The answer says it was narrowed.
    [Tags]    antares-specific    policy    adr-0020
    ${slow}=    Evaluate
    ...    {"id": $slow_id, "type": "PolicyVehicle", "speed": {"type": "Property", "value": 10}, "@context": $core}
    ${fast}=    Evaluate
    ...    {"id": $fast_id, "type": "PolicyVehicle", "speed": {"type": "Property", "value": 500}, "@context": $core}
    ${response}=    Create As    beta    ${slow}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Create As    beta    ${fast}
    Check Response Status Code    201    ${response.status_code}

    ${response}=    Query As    beta    type=PolicyVehicle
    Check Response Status Code    200    ${response.status_code}
    ${ids}=    Evaluate    [e["id"] for e in $response.json()]
    List Should Contain Value    ${ids}    ${slow_id}
    List Should Not Contain Value    ${ids}    ${fast_id}
    Dictionary Should Contain Key    ${response.headers}    ${restricted_header}

    # ...and an unrestricted tenant sees its own two, so the narrowing is the
    # rule for this tenant rather than a filter the broker runs for everyone
    ${response}=    Create As    gamma    ${fast}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Query As    gamma    type=PolicyVehicle
    Check Response Status Code    200    ${response.status_code}
    ${ids}=    Evaluate    [e["id"] for e in $response.json()]
    List Should Contain Value    ${ids}    ${fast_id}
    Dictionary Should Not Contain Key    ${response.headers}    ${restricted_header}

An Omitted Attribute Is Absent From The Answer
    [Documentation]    The other narrowing the engine asks for: `price` is
    ...    projected out of what this tenant is served. The Entity was created
    ...    whole — a filter narrows an answer, never a write — so the
    ...    Attribute is in the store and not in the response.
    [Tags]    antares-specific    policy    adr-0020
    ${response}=    Retrieve As    acme    ${priced_id}
    Check Response Status Code    200    ${response.status_code}
    Dictionary Should Not Contain Key    ${response.json()}    price
    Dictionary Should Contain Key    ${response.json()}    speed
    Dictionary Should Contain Key    ${response.headers}    ${restricted_header}

A Forbidden Attribute Is Absent From A Notification
    [Documentation]    The third narrowing: a notification is projected by the
    ...    engine before it is sent. The subscription asks for both
    ...    Attributes; the engine's omit list takes one away, and the other
    ...    still arrives — a notification the engine dropped whole would
    ...    satisfy an assertion that only looked for the absence.
    [Tags]    antares-specific    policy    adr-0020
    Start Local Server
    TRY
        ${endpoint}=    Set Variable
        ...    http://${notification_server_host}:${notification_server_port}/policy
        ${sub}=    Evaluate
        ...    {"id": $subscription_id, "type": "Subscription", "entities": [{"type": "PolicyVehicle"}], "notification": {"endpoint": {"uri": $endpoint, "accept": "application/json"}}, "@context": $core}
        ${response}=    Subscribe As    acme    ${sub}
        Check Response Status Code    201    ${response.status_code}

        ${entity}=    Evaluate
        ...    {"id": $notified_id, "type": "PolicyVehicle", "speed": {"type": "Property", "value": 7}, "price": {"type": "Property", "value": 99}, "@context": $core}
        ${response}=    Create As    acme    ${entity}
        Check Response Status Code    201    ${response.status_code}

        ${notification}    ${headers}=    Wait for notification
        ${entity}=    Set Variable    ${notification}[data][0]
        Dictionary Should Not Contain Key    ${entity}    price
        Dictionary Should Contain Key    ${entity}    speed
    FINALLY
        Stop Local Server
    END

A Subject Header Never Reaches A Context Source
    [Documentation]    The subject the engine is told about is assembled from
    ...    named request headers and stays in this process (ADR-0020). A
    ...    registration naming one in contextSourceInfo is 4.3.6.5's own
    ...    remedy for a header the broker will not convey: the pair is
    ...    ignored, and the Context Source sees the forward without it.
    [Tags]    antares-specific    policy    adr-0020    4_3_6_5
    Start Server    ${context_source_host}    ${context_source_port}
    TRY
        Set Stub Reply    GET    /ngsi-ld/v1/entities?type=PolicyRemote    200    []
        ${csi}=    Evaluate    [{"key": $subject_header, "value": "smuggled"}]
        ${reg}=    Evaluate
        ...    {"id": $registration_id, "type": "ContextSourceRegistration", "information": [{"entities": [{"type": "PolicyRemote"}]}], "endpoint": "http://" + $context_source_host + ":" + str($context_source_port), "contextSourceInfo": $csi, "@context": $core}
        ${response}=    Register As    gamma    ${reg}
        Check Response Status Code    201    ${response.status_code}

        ${response}=    Query As    gamma    type=PolicyRemote    someone
        Check Response Status Code    200    ${response.status_code}
        Wait For Request    ${15}
        ${hdrs}=    Get Request Headers
        Should Be Equal    ${{ $hdrs.get($subject_header, '') }}    ${EMPTY}
        # the header the request itself carried is not conveyed either
        Should Not Contain    ${{ str(sorted($hdrs.items())) }}    someone
        Should Not Contain    ${{ str(sorted($hdrs.items())) }}    smuggled
    FINALLY
        Stop Server
    END


*** Keywords ***
Mint The Suite Ids
    [Documentation]    One suffix per run, so a re-run against a broker that
    ...    kept its state does not collide with the previous one.
    ${suffix}=    Evaluate    __import__("uuid").uuid4().hex[:10]
    Set Suite Variable    ${suffix}
    Set Suite Variable    ${slow_id}    urn:ngsi-ld:PolicyVehicle:slow-${suffix}
    Set Suite Variable    ${fast_id}    urn:ngsi-ld:PolicyVehicle:fast-${suffix}
    Set Suite Variable    ${priced_id}    urn:ngsi-ld:PolicyVehicle:priced-${suffix}
    Set Suite Variable    ${notified_id}    urn:ngsi-ld:PolicyVehicle:notified-${suffix}
    Set Suite Variable    ${subscription_id}    urn:ngsi-ld:Subscription:policy-${suffix}
    Set Suite Variable
    ...    ${registration_id}
    ...    urn:ngsi-ld:ContextSourceRegistration:policy-${suffix}

Tenant Headers
    [Arguments]    ${tenant}    ${subject}=${None}
    ${headers}=    Create Dictionary    NGSILD-Tenant=${tenant}
    IF    $subject is not None
        Set To Dictionary    ${headers}    ${subject_header}    ${subject}
    END
    RETURN    ${headers}

Create As
    [Arguments]    ${tenant}    ${payload}
    ${headers}=    Tenant Headers    ${tenant}
    Set To Dictionary    ${headers}    Content-Type    application/ld+json
    ${response}=    POST
    ...    url=${url}/entities
    ...    json=${payload}
    ...    headers=${headers}
    ...    expected_status=any
    RETURN    ${response}

Subscribe As
    [Arguments]    ${tenant}    ${payload}
    ${headers}=    Tenant Headers    ${tenant}
    Set To Dictionary    ${headers}    Content-Type    application/ld+json
    ${response}=    POST
    ...    url=${url}/subscriptions
    ...    json=${payload}
    ...    headers=${headers}
    ...    expected_status=any
    RETURN    ${response}

Register As
    [Arguments]    ${tenant}    ${payload}
    ${headers}=    Tenant Headers    ${tenant}
    Set To Dictionary    ${headers}    Content-Type    application/ld+json
    ${response}=    POST
    ...    url=${url}/csourceRegistrations
    ...    json=${payload}
    ...    headers=${headers}
    ...    expected_status=any
    RETURN    ${response}

Query As
    [Arguments]    ${tenant}    ${query}    ${subject}=${None}
    ${headers}=    Tenant Headers    ${tenant}    ${subject}
    ${response}=    GET    url=${url}/entities?${query}    headers=${headers}    expected_status=any
    RETURN    ${response}

Retrieve As
    [Arguments]    ${tenant}    ${entity_id}
    ${headers}=    Tenant Headers    ${tenant}
    ${response}=    GET    url=${url}/entities/${entity_id}    headers=${headers}    expected_status=any
    RETURN    ${response}
