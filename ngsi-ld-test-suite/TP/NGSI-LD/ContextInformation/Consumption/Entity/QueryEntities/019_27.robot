*** Settings ***
Documentation       Check the 5.7.2.4 validation bullets for `geometryProperty`, and that
...                 ordering is accepted when the operation executes locally.
...
...                 5.7.2.4 (p.201): "If geometryProperty parameter is present and the
...                 Accept Header is not set to \"application/geo+json\", then an error of
...                 type BadRequestData shall be raised."
...
...                 The companion ordering rule from the same clause — "If the ordering
...                 parameter is present and the execution of the operation is not limited
...                 to the local scope … BadRequestData" — is about the *execution*, so it
...                 only bites when a registration matches. It is covered in
...                 DistributedOperations/Consumption/Entity/QueryEntities/D011_05_inc;
...                 here we pin the converse, that a purely local ordered query is legal.
...
...                 Antares extension TP.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Suite Setup         Create One Building
Suite Teardown      Delete Entity    ${entity_id}


*** Test Cases ***
019_27_01 Query Entities With GeometryProperty And Non GeoJSON Accept
    [Documentation]    geometryProperty outside application/geo+json must be
    ...    BadRequestData (5.7.2.4)
    [Tags]    e-query    6_3_15    5_7_2    6_4_3_2    since_v1.9.1

    ${response}=    Query Entities
    ...    entity_types=Building
    ...    geometryProperty=location
    ...    accept=${CONTENT_TYPE_JSON}
    ...    context=${ngsild_test_suite_context}

    Bad Request Data    ${response}

019_27_02 Query Entities With GeometryProperty And GeoJSON Accept Is Accepted
    [Documentation]    With the GeoJSON Accept header the same parameter is legal
    [Tags]    e-query    6_3_15    5_7_2    6_4_3_2    since_v1.9.1

    ${response}=    Query Entities
    ...    entity_types=Building
    ...    geometryProperty=location
    ...    accept=application/geo+json
    ...    context=${ngsild_test_suite_context}

    Check Response Status Code    200    ${response.status_code}

019_27_03 Query Entities With OrderBy Under Local Execution Is Accepted
    [Documentation]    With no matching registration the operation executes locally, so
    ...    4.23 ordering applies — with or without an explicit local=true
    [Tags]    e-query    4_23    5_7_2    6_4_3_2    since_v1.9.1

    ${response}=    Query Entities
    ...    entity_types=Building
    ...    orderBy=name
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}

    ${response}=    Query Entities
    ...    entity_types=Building
    ...    orderBy=name
    ...    local=true
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}


*** Keywords ***
Create One Building
    ${entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    building-simple-attributes.jsonld
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}

Bad Request Data
    [Arguments]    ${response}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
