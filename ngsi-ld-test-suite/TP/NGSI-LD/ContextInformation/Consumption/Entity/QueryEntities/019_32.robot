*** Settings ***
Documentation       Check the Query data type (CIM 009 clause 5.2.23, Table 5.2.23-1) on
...                 POST /entityOperations/query: body members q/pick/omit are honoured
...                 like their query-parameter twins; empty arrays (entities, attrs, pick)
...                 are not allowed; string members must be strings; joinLevel is a
...                 positive integer; temporalQ is to be present only for the "Query
...                 Temporal Evolution of Entities" operation.
...
...                 Antares extension TP — the official 019_02_x TPs only exercise valid
...                 entities/attrs members.

Library             RequestsLibrary
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Suite Setup         Setup Query Entities
Suite Teardown      Delete Query Entities


*** Variables ***
${slow_vehicle}=    urn:ngsi-ld:Vehicle:01932-A
${fast_vehicle}=    urn:ngsi-ld:Vehicle:01932-B


*** Test Cases ***
019_32_01 Body Q Filters Like The Query Parameter
    [Documentation]    5.2.23: q — "Query that shall be matched by Entities in order to
    ...    be retrieved"
    [Tags]    e-query    5_2_23    since_v1.9.1
    Post Query Expecting    200    {"type": "Query", "entities": [{"type": "Vehicle"}], "q": "speed>100"}
    ...    ${fast_vehicle}    ${slow_vehicle}

019_32_02 Body Pick Projects The Entity
    [Documentation]    5.2.23: pick — "every Entity within the payload body is reduced
    ...    down to only contain the specified Entity members"
    [Tags]    e-query    5_2_23    since_v1.9.1
    Post Query Expecting    200    {"type": "Query", "entities": [{"type": "Vehicle"}], "pick": ["speed"]}
    ...    speed    brand

019_32_03 Body Omit Removes Members
    [Documentation]    5.2.23: omit — "the specified Entity members are removed from
    ...    each Entity within the payload"
    [Tags]    e-query    5_2_23    since_v1.9.1
    Post Query Expecting    200    {"type": "Query", "entities": [{"type": "Vehicle"}], "omit": ["speed"]}
    ...    brand    speed

019_32_04 An Empty Entities Array Is Rejected
    [Documentation]    Table 5.2.23-1: entities — "Empty array (0 length) is not allowed"
    [Tags]    e-query    5_2_23    since_v1.9.1
    Post Query Expecting    400    {"type": "Query", "entities": []}    ${None}    ${None}

019_32_05 An Empty Attrs Array Is Rejected
    [Documentation]    Table 5.2.23-1: attrs — "Empty array (0 length) is not allowed"
    [Tags]    e-query    5_2_23    since_v1.9.1
    Post Query Expecting    400    {"type": "Query", "entities": [{"type": "Vehicle"}], "attrs": []}
    ...    ${None}    ${None}

019_32_06 A Non-String Q Is Rejected
    [Documentation]    Table 5.2.23-1: q is a String
    [Tags]    e-query    5_2_23    since_v1.9.1
    Post Query Expecting    400    {"type": "Query", "entities": [{"type": "Vehicle"}], "q": 42}
    ...    ${None}    ${None}

019_32_07 JoinLevel Zero Is Rejected
    [Documentation]    Table 5.2.23-1: joinLevel — "Positive Integer"
    [Tags]    e-query    5_2_23    since_v1.9.1
    Post Query Expecting    400
    ...    {"type": "Query", "entities": [{"type": "Vehicle"}], "join": "inline", "joinLevel": 0}
    ...    ${None}    ${None}

019_32_08 TemporalQ Is Only For The Temporal Operation
    [Documentation]    Table 5.2.23-1: temporalQ — "to be present only for Query Temporal
    ...    Evolution of Entities operation (clause 5.7.4)"
    [Tags]    e-query    5_2_23    since_v1.9.1
    Post Query Expecting    400
    ...    {"type": "Query", "entities": [{"type": "Vehicle"}], "temporalQ": {"timerel": "after", "timeAt": "2020-01-01T00:00:00Z"}}
    ...    ${None}    ${None}

019_32_09 A Body Type Other Than Query Is Rejected
    [Documentation]    Table 5.2.23-1: type — "It shall be equal to Query"
    [Tags]    e-query    5_2_23    since_v1.9.1
    Post Query Expecting    400    {"type": "NotQuery", "entities": [{"type": "Vehicle"}]}
    ...    ${None}    ${None}


*** Keywords ***
Post Query Expecting
    [Arguments]    ${expected_status_code}    ${query_body}    ${must_contain}    ${must_not_contain}
    ${payload}=    Evaluate    json.loads($query_body)    modules=json
    ${response}=    POST
    ...    url=${url}/${ENTITY_OPERATIONS_QUERY_ENDPOINT_PATH}
    ...    json=${payload}
    ...    headers=${{ {"Content-Type": "application/json"} }}
    ...    expected_status=any
    Check Response Status Code    ${expected_status_code}    ${response.status_code}
    IF    $must_contain is not None
        Should Contain    ${response.text}    ${must_contain}
    END
    IF    $must_not_contain is not None
        Should Not Contain    ${response.text}    ${must_not_contain}
    END

Setup Query Entities
    FOR    ${id}    ${speed}    IN    ${slow_vehicle}    ${80}    ${fast_vehicle}    ${120}
        ${payload}=    Evaluate
        ...    json.dumps({"id": $id, "type": "Vehicle", "speed": {"type": "Property", "value": $speed}, "brand": {"type": "Property", "value": "Mercedes"}})
        ...    modules=json
        ${response}=    POST
        ...    url=${url}/${ENTITIES_ENDPOINT_PATH}
        ...    data=${payload}
        ...    headers=${{ {"Content-Type": "application/json"} }}
        ...    expected_status=any
        Check Response Status Code    201    ${response.status_code}
    END

Delete Query Entities
    FOR    ${id}    IN    ${slow_vehicle}    ${fast_vehicle}
        ${response}=    DELETE
        ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${id}
        ...    expected_status=any
    END
