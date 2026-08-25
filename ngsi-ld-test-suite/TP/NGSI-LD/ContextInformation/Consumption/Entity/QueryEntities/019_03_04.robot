*** Settings ***
Documentation       Check that one cannot query entities if the request has a wrong geometryProperty

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Create Entities
Suite Teardown      Delete Entities


*** Variables ***
${filename}=        building-location-attribute.jsonld
${entity_type}=     Building
${georel}=          invalid
${geometry}=        Point
${coordinates}=     [-8.503,41.202]
${geoproperty}=     location


*** Test Cases ***
019_03_04 Query Entities When The Request Has A Wrong geometryProperty
    [Documentation]    Check that one cannot query entities if the request has a wrong geometryProperty
    [Tags]    e-query    5_7_2
    ${entity_types_to_be_retrieved}=    Catenate    SEPARATOR=,    ${entity_type}

    ${response}=    Query Entities
    ...    entity_types=${entity_types_to_be_retrieved}
    ...    geoproperty=${geoproperty}
    ...    georel=${georel}
    ...    geometry=${geometry}
    ...    coordinates=${coordinates}
    ...    context=${ngsild_test_suite_context}

    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}


*** Keywords ***
Create Entities
    ${first_entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${first_entity_id}
    ${create_response1}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${first_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response1.status_code}
    ${second_entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${second_entity_id}
    ${create_response2}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${second_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response2.status_code}

Delete Entities
    Delete Entity    ${first_entity_id}
    Delete Entity    ${second_entity_id}
