*** Settings ***
Documentation       Verify that one instant is one temporal instance, however the
...                 client spelled its observedAt.
...
...                 ANTARES DECISION, NOT A CIM 009 REQUIREMENT. 4.5.7 describes
...                 an instance as the Property "at a particular point in time,
...                 which is recorded as a Temporal Property of the instance
...                 (typically observedAt)", and 4.6.3 lets a client spell one
...                 instant several ways: the seconds fraction is optional, and
...                 "In requests, also a comma instead of a decimal point may be
...                 used as separator". But 4.5.7 only says systems "should
...                 maintain an instanceId", so how a broker derives instance
...                 identity is its own decision, and this test would fail a
...                 broker that derives it some other way.
...
...                 The reason is the one 4.5.7 names itself: "Without such an
...                 instanceId, it is not possible to selectively modify or
...                 delete temporal information via the NGSI-LD API. The
...                 consequences of this may be severe in the case of
...                 modification or deletion requests for legal reasons." Antares
...                 derives the id of an auto-recorded instance from the instant
...                 rather than from its spelling, so a correction re-sent for
...                 the same instant replaces the value it is correcting instead
...                 of landing beside it.
...
...                 This applies to instances recorded from Core API writes. The
...                 temporal API itself is add-only by 5.6.11.4 and 5.6.12.1
...                 ("by adding new Attribute instances"), so a re-post there
...                 appends, as the clause says it should.

Library             RequestsLibrary
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource


*** Test Cases ***
A Correction At The Same Instant Replaces The Instance It Corrects
    [Documentation]    Two Core API writes for one instant, the second spelling
    ...    the seconds fraction and using a comma separator: the temporal
    ...    evolution holds ONE instance for that instant, carrying the corrected
    ...    value.
    [Tags]    e-create    e-update    t-retrieve    4_5_7
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "speed": {"type": "Property", "value": 10, "observedAt": "2020-01-01T00:00:00Z"}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    201    ${response.status_code}
    ${fragment}=    Evaluate
    ...    {"@context": [$ngsild_test_suite_context], "speed": {"type": "Property", "value": 42, "observedAt": "2020-01-01T00:00:00,000Z"}}
    &{headers}=    Create Dictionary    Content-Type=application/ld+json
    ${response}=    POST
    ...    url=${url}/entities/${entity_id}/attrs/
    ...    json=${fragment}
    ...    headers=${headers}
    ...    expected_status=any
    Should Contain    ${{[204, 207]}}    ${response.status_code}
    ${response}=    Retrieve Temporal Representation Of Entity
    ...    ${entity_id}
    ...    timerel=between
    ...    timeAt=2019-01-01T00:00:00Z
    ...    endTimeAt=2021-01-01T00:00:00Z
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${instances}=    Evaluate    $response.json()['speed']
    ${at_instant}=    Evaluate
    ...    [i for i in $instances if i.get('observedAt', '').startswith('2020-01-01T00:00:00')]
    Length Should Be    ${at_instant}    1    one instant is one instance
    Should Be Equal As Numbers    ${at_instant}[0][value]    42
    [Teardown]    Delete Entity    ${entity_id}
