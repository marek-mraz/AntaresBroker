*** Settings ***
Documentation       Check that one can query context source registrations. If present, the geoquery is matched against the GeoProperty programmatic parameter identified in the geoquery

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Context Source Registration
Test Teardown       Delete Created Context Source Registration
Test Template       Query Context Source Registration Matching Geoquery


*** Variables ***
${context_source_registration_payload_file_path}=       csourceRegistrations/context-source-registration-location.jsonld
${expectation_file_path}=                               csourceRegistrations/expectations/context-source-registrations-037-07.json


*** Test Cases ***    GEOREL    GEOMETRY    COORDINATES    GEOPROPERTY    EXPECTATION_FILE_PATH
037_07_01 Near Point
    [Tags]    csr-query    5_10_2
    near;maxDistance==2000    Point    [-8.503,41.202]    ${EMPTY}    ${expectation_file_path}
037_07_02 Within Polygon
    [Tags]    csr-query    5_10_2
    within    Polygon    [[[-13.503,47.202],[6.541,52.961],[20.37,44.653],[9.46,32.57],[-15.23,21.37],[-13.503,47.202]]]    location    ${expectation_file_path}


*** Keywords ***
Query Context Source Registration Matching Geoquery
    [Documentation]    Check that one can query context source registrations. If present, the geoquery is matched against the GeoProperty programmatic parameter identified in the geoquery
    [Arguments]    ${georel}    ${geometry}    ${coordinates}    ${geoproperty}    ${expectation_file_path}
    ${response}=    Query Context Source Registrations
    ...    context=${ngsild_test_suite_context}
    ...    type=Building
    ...    georel=${georel}
    ...    geometry=${geometry}
    ...    coordinates=${coordinates}
    ...    geoproperty=${geoproperty}
    @{expected_context_source_registration_ids}=    Create List    ${context_source_registration_id}
    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing List Containing Context Source Registrations elements
    ...    ${expectation_file_path}
    ...    ${expected_context_source_registration_ids}
    ...    ${response.json()}

Setup Initial Context Source Registration
    ${context_source_registration_id}=    Generate Random CSR Id
    ${context_source_registration_payload}=    Load Test Sample
    ...    ${context_source_registration_payload_file_path}
    ...    ${context_source_registration_id}
    ${create_response}=    Create Context Source Registration    ${context_source_registration_payload}
    Check Response Status Code    201    ${create_response.status_code}
    Set Test Variable    ${context_source_registration_id}

Delete Created Context Source Registration
    Delete Context Source Registration    ${context_source_registration_id}
