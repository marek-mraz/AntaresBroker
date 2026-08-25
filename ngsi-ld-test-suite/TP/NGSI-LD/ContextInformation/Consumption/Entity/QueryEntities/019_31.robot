*** Settings ***
Documentation       Check the pagination behaviour of Query Entities (CIM 009 clause 4.12):
...                 a client-specified limit (page size), a flag for remaining elements
...                 (next link), backwards iteration (prev link), and the absence of each
...                 link at the respective edge of the result set.
...
...                 Antares extension TP — the official pagination-link assertions cover
...                 subscriptions (031_02), registrations (037_11) and csource
...                 subscriptions (041_03), but not the entity list itself.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Entities
Suite Teardown      Delete Initial Entities
Test Template       Query Entities Page


*** Variables ***
${entity_filename}=     building-simple-attributes.jsonld
${id_prefix}=           urn:ngsi-ld:Building:01931
${id_pattern}=          urn:ngsi-ld:Building:01931.*
${entities_path}=       /ngsi-ld/v1/entities


*** Test Cases ***    LIMIT    OFFSET    EXPECTED_COUNT_IN_PAGE    PREV_LINK    NEXT_LINK
019_31_01 First Page Flags Remaining Elements With Next Only
    [Documentation]    4.12: "provide a mechanism to flag NGSI-LD Clients when there are
    ...    remaining NGSI-LD Elements" — and no prev on the first page
    [Tags]    e-query    4_12    5_5_9_1    5_5_9_2    since_v1.9.1
    1    0    1
    ...    ${EMPTY}
    ...    <${entities_path}?count=true&idPattern=${id_pattern}&limit=1&offset=1&type=Building>; rel="next";type="application/json"

019_31_02 Middle Page Carries Both Prev And Next
    [Documentation]    4.12: "allow NGSI-LD Clients iterating forwards and backwards
    ...    through a result set"
    [Tags]    e-query    4_12    5_5_9_1    5_5_9_2    since_v1.9.1
    1    1    1
    ...    <${entities_path}?count=true&idPattern=${id_pattern}&limit=1&offset=0&type=Building>; rel="prev";type="application/json"
    ...    <${entities_path}?count=true&idPattern=${id_pattern}&limit=1&offset=2&type=Building>; rel="next";type="application/json"

019_31_03 Last Page Carries Prev Only
    [Documentation]    4.12: no remaining elements — the next link must be absent
    [Tags]    e-query    4_12    5_5_9_1    5_5_9_2    since_v1.9.1
    1    2    1
    ...    <${entities_path}?count=true&idPattern=${id_pattern}&limit=1&offset=1&type=Building>; rel="prev";type="application/json"
    ...    ${EMPTY}

019_31_04 Client Page Size Bounds The Page
    [Documentation]    4.12: "allow NGSI-LD Clients specifying a limit (page size) ... to
    ...    the number of NGSI-LD Elements (at a maximum) retrieved ... per pagination
    ...    iteration" — 2 of 3 on the first page, the remainder flagged via next
    [Tags]    e-query    4_12    5_5_9_1    5_5_9_2    since_v1.9.1
    2    0    2
    ...    ${EMPTY}
    ...    <${entities_path}?count=true&idPattern=${id_pattern}&limit=2&offset=2&type=Building>; rel="next";type="application/json"

019_31_05 Limit Zero With Count Returns The Total And No Elements
    [Documentation]    4.13: "a client can issue a query that limits to zero the number
    ...    of desired results but asks for the count to be present" — empty page, count
    ...    header, no pagination links
    [Tags]    e-query    4_13    since_v1.9.1
    0    0    0    ${EMPTY}    ${EMPTY}


*** Keywords ***
Query Entities Page
    [Arguments]    ${limit}    ${offset}    ${expected_in_page}    ${prev_link}    ${next_link}
    ${response}=    Query Entities
    ...    entity_types=Building
    ...    entity_id_pattern=${id_pattern}
    ...    limit=${limit}
    ...    offset=${offset}
    ...    count=true
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    Check Response Headers Containing NGSILD-Results-Count Equals To    3    ${response.headers}
    ${page_size}=    Evaluate    len($response.json())
    Should Be Equal As Integers    ${page_size}    ${expected_in_page}
    Check Pagination Prev And Next Headers    ${prev_link}    ${next_link}    ${response.headers}

Setup Initial Entities
    FOR    ${suffix}    IN    A    B    C
        ${entity_id}=    Catenate    SEPARATOR=    ${id_prefix}    ${suffix}
        ${create_response}=    Create Entity Selecting Content Type
        ...    ${entity_filename}
        ...    ${entity_id}
        ...    ${CONTENT_TYPE_LD_JSON}
        Check Response Status Code    201    ${create_response.status_code}
    END

Delete Initial Entities
    FOR    ${suffix}    IN    A    B    C
        ${entity_id}=    Catenate    SEPARATOR=    ${id_prefix}    ${suffix}
        Delete Entity    ${entity_id}
    END
