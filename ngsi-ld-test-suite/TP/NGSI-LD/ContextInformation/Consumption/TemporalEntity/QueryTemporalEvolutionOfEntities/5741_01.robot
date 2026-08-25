*** Settings ***
Documentation       Verify 5.7.4.4 Query Temporal Evolution validation edges
...                 the official 021 TPs skip.
...
...                 5.7.4.4: attrs/q qualify the too-wide guard only with a
...                 non-system Attribute; Linked Entity projection or filter
...                 conditions are an unconditional BadRequestData; an
...                 invalid id URI or context source filter is
...                 BadRequestData; the ordering parameter may only refer to
...                 the "id" entity member.
...
...                 Antares extension TP.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Test Template       Query Temporal Entities Expecting BadRequestData


*** Variables ***
${window}=      timerel=after&timeAt=2020-01-01T00:00:00Z


*** Test Cases ***    QUERY_STRING
5741_01_01 System Attrs Alone Are Too Wide
    [Tags]    e-query-temporal    5_7_4    since_v1.9.1
    ${window}&attrs=createdAt

5741_01_02 Linked Projection Is BadRequestData
    [Tags]    e-query-temporal    5_7_4    since_v1.9.1
    ${window}&type=Building&pick=owner{name}

5741_01_03 Linked Filter Is BadRequestData
    [Tags]    e-query-temporal    5_7_4    since_v1.9.1
    ${window}&type=Building&q=owner{name}=="x"

5741_01_04 Invalid Id URI Is BadRequestData
    [Tags]    e-query-temporal    5_7_4    since_v1.9.1
    ${window}&type=Building&id=not a uri

5741_01_05 Invalid Context Source Filter Is BadRequestData
    [Tags]    e-query-temporal    5_7_4    since_v1.9.1
    ${window}&type=Building&csf=))bad((

5741_01_06 OrderBy Other Than Id Is BadRequestData
    [Tags]    e-query-temporal    5_7_4    since_v1.9.1
    ${window}&type=Building&orderBy=name


*** Keywords ***
Query Temporal Entities Expecting BadRequestData
    [Arguments]    ${query_string}
    &{headers}=    Create Dictionary    Accept=application/ld+json
    ${response}=    GET
    ...    url=${temporal_api_url}/${TEMPORAL_ENTITIES_ENDPOINT_PATH}?${query_string}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    response_body=${response.json()}
    ...    type=${ERROR_TYPE_BAD_REQUEST_DATA}
