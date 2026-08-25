*** Settings ***
Documentation       Verify 5.7.2.4 Query Entities validation edges the
...                 official 019 TPs skip.
...
...                 5.7.2.4: the attrs list / q qualify the too-wide guard
...                 only when they include "at least one non-system
...                 Attribute"; filter conditions using Linked Entity
...                 attributes without join (or deeper than joinLevel) are
...                 BadRequestData ("too deep query"); a syntactically
...                 invalid context source filter is BadRequestData.
...
...                 Antares extension TP.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Test Template       Query Entities Expecting BadRequestData


*** Test Cases ***    QUERY_STRING
5722_01_01 System Attrs Alone Are Too Wide
    [Documentation]    5.7.2.4 b: attrs listing only system names does not
    ...    qualify — too-wide 400.
    [Tags]    e-query    5_7_2    since_v1.9.1
    attrs=createdAt

5722_01_02 System Q Alone Is Too Wide
    [Documentation]    5.7.2.4 c: a q referencing only system attributes
    ...    does not qualify — too-wide 400.
    [Tags]    e-query    5_7_2    since_v1.9.1
    q=modifiedAt>"2020-01-01T00:00:00Z"

5722_01_03 Linked Q Without Join Is Too Deep
    [Documentation]    5.7.2.4: q referencing Linked Entity attributes while
    ...    "the use of Linked Entity retrieval is not specified" → 400.
    [Tags]    e-query    5_7_2    since_v1.9.1
    type=Building&q=owner{name}=="x"

5722_01_04 Linked Q Deeper Than JoinLevel Is Too Deep
    [Documentation]    5.7.2.4: "the Linked Entity attribute query depth
    ...    exceeds the Linked Entity retrieval depth" → 400.
    [Tags]    e-query    5_7_2    since_v1.9.1
    type=Building&q=owner{works{name}}=="x"&join=inline&joinLevel=1

5722_01_05 Invalid Context Source Filter Is BadRequestData
    [Documentation]    5.7.2.4: "the query, geoquery or context source
    ...    filter are not syntactically valid" → 400.
    [Tags]    e-query    5_7_2    since_v1.9.1
    type=Building&csf=))bad((


*** Keywords ***
Query Entities Expecting BadRequestData
    [Arguments]    ${query_string}
    &{headers}=    Create Dictionary    Accept=application/ld+json
    ${response}=    GET
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}?${query_string}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    response_body=${response.json()}
    ...    type=${ERROR_TYPE_BAD_REQUEST_DATA}
