*** Settings ***
Documentation       Check that one can query entities based on a geoquery

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Entities
Suite Teardown      Delete Entities


*** Variables ***
${filename}=                building-location-attribute.jsonld
${expectation_filename}=    building-geoproperty-query.jsonld
${entity_type}=             https://ngsi-ld-test-suite/context#Building
${georel}=                  equals
${geometry}=                Point
${coordinates}=             [13.3986, 52.5547]


*** Test Cases ***
019_01_05 Query Several Entities Based On A Geoquery
    [Documentation]    Check that one can query entities based on a geoquery
    [Tags]    e-query    5_7_2
    @{entities_ids_to_be_compared}=    Create List    ${first_entity_id}    ${second_entity_id}
    ${entity_types_to_be_retrieved}=    Catenate    SEPARATOR=,    ${entity_type}

    ${response}=    Query Entities
    ...    entity_types=${entity_types_to_be_retrieved}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    ...    georel=${georel}
    ...    geometry=${geometry}
    ...    coordinates=${coordinates}

    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing List Containing Entity Elements
    ...    ${expectation_filename}
    ...    ${entities_ids_to_be_compared}
    ...    ${response.json()}
    ...    ${True}


*** Keywords ***
Setup Initial Entities
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
